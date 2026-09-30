use crate::{
    error::{AppError, Result},
    model::TransportPolicy,
};
use std::collections::BTreeMap;
use url::Url;
#[derive(Clone)]
pub struct ApiBase(Url);
impl ApiBase {
    pub fn parse(input: &str, policy: TransportPolicy) -> Result<Self> {
        if input.trim() != input
            || input.contains(['\\', '%', '?', '#'])
            || input.chars().any(char::is_control)
        {
            return Err(AppError::input());
        }
        let authority = input
            .split_once("://")
            .ok_or_else(AppError::input)?
            .1
            .split('/')
            .next()
            .unwrap_or("");
        if authority.contains('@') {
            return Err(AppError::input());
        }
        let raw_host = if authority.starts_with('[') {
            authority
                .split(']')
                .next()
                .unwrap_or("")
                .trim_start_matches('[')
        } else {
            authority.split(':').next().unwrap_or("")
        };
        let literal_loopback = raw_host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|a| a.is_loopback());
        let mut u = Url::parse(input).map_err(|_| AppError::input())?;
        let raw_path = input
            .split_once("://")
            .ok_or_else(AppError::input)?
            .1
            .split_once('/')
            .map(|(_, p)| p)
            .unwrap_or("");
        if raw_path
            .split('/')
            .any(|s| s == "." || s == ".." || s.is_empty() && raw_path.contains("//"))
            || !u.username().is_empty()
            || u.password().is_some()
            || u.host_str().is_none()
        {
            return Err(AppError::input());
        }
        let loopback = match u.host() {
            Some(url::Host::Ipv4(a)) => a.is_loopback(),
            Some(url::Host::Ipv6(a)) => a.is_loopback(),
            _ => false,
        };
        if u.scheme() != "https"
            && !(u.scheme() == "http"
                && loopback
                && literal_loopback
                && policy == TransportPolicy::LoopbackDevelopment)
        {
            return Err(AppError::new(
                "HTTPS_REQUIRED",
                "Use HTTPS. Literal loopback HTTP is available only with explicit development enrollment.",
            ));
        }
        let path = u.path().trim_end_matches('/');
        let path = if path.ends_with("/api") {
            path.to_string()
        } else {
            format!("{path}/api")
        };
        u.set_path(&path);
        Ok(Self(u))
    }
    pub fn path(&self) -> &str {
        self.0.path()
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn route(&self, template: &'static str, params: &BTreeMap<String, String>) -> Result<Url> {
        if !template.starts_with('/') || template.contains(['?', '#', '%', '\\']) {
            return Err(AppError::input());
        }
        let mut u = self.0.clone();
        {
            let mut segments = u.path_segments_mut().map_err(|_| AppError::input())?;
            for part in template[1..].split('/') {
                let value = if part.starts_with('{') && part.ends_with('}') {
                    params
                        .get(&part[1..part.len() - 1])
                        .ok_or_else(AppError::input)?
                        .as_str()
                } else {
                    part
                };
                if value.is_empty()
                    || matches!(value, "." | "..")
                    || value.chars().any(char::is_control)
                {
                    return Err(AppError::input());
                }
                segments.push(value);
            }
        }
        if u.origin() != self.0.origin() || !u.path().starts_with(&format!("{}/", self.path())) {
            return Err(AppError::input());
        }
        Ok(u)
    }
}
