use hermes_grocy_mcp::{
    catalog,
    model::{AccessMode, Page, RecordId},
    tools::validation::{Conversion, RecordSelector, convert_quantity, resolve_record},
};
use rust_decimal::Decimal;
use serde_json::json;
#[test]
fn household_catalog_and_validation() {
    assert!(catalog::operations(AccessMode::Full).len() > 40);
    assert!(
        catalog::operations(AccessMode::ReadOnly)
            .iter()
            .all(|o| !o.mutates)
    );
    for entity in ["api_keys", "sessions", "permission_hierarchy", "users"] {
        assert!(
            catalog::validate("records_list", json!({"entity":entity}), AccessMode::Full).is_err()
        );
    }
    assert!(
        catalog::validate(
            "records_update",
            json!({"entity":"stock_log","id":1,"fields":{"amount":2}}),
            AccessMode::Full
        )
        .is_err()
    );
    assert!(
        catalog::validate(
            "records_create",
            json!({"entity":"products","fields":{"name":"Sample","id":4}}),
            AccessMode::Full
        )
        .is_err()
    );
    let rows = vec![
        json!({"id":"1","name":"Milk"}),
        json!({"id":"2","name":"Milk"}),
    ];
    assert_eq!(
        resolve_record(&rows, &RecordSelector::ExactName("Milk".into()))
            .unwrap_err()
            .code,
        "AMBIGUOUS_RECORD"
    );
    assert_eq!(
        convert_quantity(
            Decimal::from(500),
            RecordId(1),
            RecordId(2),
            &[Conversion {
                from: RecordId(1),
                to: RecordId(2),
                factor: "0.001".parse().unwrap()
            }]
        )
        .unwrap()
        .to_string(),
        "0.500"
    );
    assert_eq!(
        convert_quantity(Decimal::ONE, RecordId(1), RecordId(2), &[])
            .unwrap_err()
            .code,
        "UNIT_CONVERSION_REQUIRED"
    );
    assert!(
        catalog::validate(
            "stock_add",
            json!({"product_id":1,"amount":-1}),
            AccessMode::Full
        )
        .is_err()
    );
    assert!(
        catalog::validate(
            "stock_add",
            json!({"product_id":1,"amount":1,"best_before_date":"2026-02-30"}),
            AccessMode::Full
        )
        .is_err()
    );
    assert!(
        catalog::validate(
            "stock_inventory",
            json!({"product_id":1,"new_amount":0}),
            AccessMode::Full
        )
        .is_ok()
    );
    assert_eq!(Page::default().limit, 100);
    assert!(Page::new(0, 501).is_err());
}

#[tokio::test]
async fn stock_unit_change_requires_conversion() {
    use axum::{Json, Router};
    use hermes_grocy_mcp::{
        client::{ApiBase, ApiClient},
        model::{PrivateProfile, TransportPolicy},
        tools::records,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let conversion = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicUsize::new(0));
    let conv = conversion.clone();
    let written = writes.clone();
    let app = Router::new().fallback(move |req: axum::extract::Request| {
        let conv = conv.clone();
        let written = written.clone();
        async move {
            if req.method() == "PUT" {
                written.fetch_add(1, Ordering::SeqCst);
                return Json(json!({}));
            }
            match req.uri().path() {
                "/api/objects/products/7" => Json(
                    json!({"id":7,"qu_id_stock":if written.load(Ordering::SeqCst)>0{2}else{1}}),
                ),
                "/api/objects/quantity_unit_conversions_resolved" => {
                    Json(if conv.load(Ordering::SeqCst) {
                        json!([{"product_id":7,"from_qu_id":1,"to_qu_id":2,"factor":"0.001"}])
                    } else {
                        json!([])
                    })
                }
                _ => Json(json!({"id":2})),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ApiClient::new(PrivateProfile {
        api_base: ApiBase::parse(
            &format!("http://127.0.0.1:{port}"),
            TransportPolicy::LoopbackDevelopment,
        )
        .unwrap(),
        api_key: secrecy::SecretString::from("synthetic-key"),
        access: AccessMode::Full,
        timezone: None,
        defaults: json!(null),
        ca_path: None,
        transport: TransportPolicy::LoopbackDevelopment,
        unsafe_storage: false,
    })
    .unwrap();
    let call = catalog::validate(
        "records_update",
        json!({"entity":"products","id":7,"fields":{"qu_id_stock":2}}),
        AccessMode::Full,
    )
    .unwrap();
    assert_eq!(
        records::run(&client, &call).await.err().unwrap().code,
        "UNIT_CONVERSION_REQUIRED"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    conversion.store(true, Ordering::SeqCst);
    assert!(records::run(&client, &call).await.is_ok());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    server.abort();
}
