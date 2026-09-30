#!/usr/bin/env python3
"""Own a disposable synthetic Grocy fixture. Never use an existing household."""
import argparse, json, os, pathlib, secrets, subprocess, time, urllib.request
IMAGE = "lscr.io/linuxserver/grocy@sha256:3c1ce96cb2d0f0c7ea0c2e0b89f5769be7f2f102204fbeef87aeef56afd6036e"
LABEL = "io.github.hermes-grocy-mcp.contract"

def docker(*args, input=None):
    return subprocess.check_output(["docker", *args], input=input, stderr=subprocess.DEVNULL).decode().strip()

def load(state):
    p = state / "fixture.json"
    if p.is_symlink() or p.stat().st_uid != os.getuid() or p.stat().st_mode & 0o077:
        raise RuntimeError("Unsafe fixture manifest")
    return json.loads(p.read_text())

def start(state):
    state.mkdir(mode=0o700, parents=True, exist_ok=False)
    ident = secrets.token_hex(6)
    name = "hermes-grocy-contract-" + ident
    data = {"name":name,"network":name,"volume":name,"image":IMAGE,"api_key":secrets.token_hex(32)}
    manifest=state / "fixture.json"
    fd=os.open(manifest, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd,"w") as f: json.dump(data,f)
    try:
        docker("network","create","--label",LABEL+"="+ident,name)
        docker("volume","create","--label",LABEL+"="+ident,name)
        docker("run","--detach","--name",name,"--label",LABEL+"="+ident,"--network",name,"-p","127.0.0.1::80","-v",name+":/config",IMAGE)
        port=docker("port",name,"80/tcp").rsplit(":",1)[1]
        data["url"]="http://127.0.0.1:"+port
        # Fixture bootstrap may visit the UI for migrations. The MCP never does.
        for _ in range(90):
            try:
                urllib.request.urlopen(data["url"]+"/",timeout=2).read(256)
                break
            except Exception: time.sleep(0.5)
        code = """$p=new PDO("sqlite:/config/data/grocy.db");$k=trim(stream_get_contents(STDIN));$s=$p->prepare("INSERT INTO api_keys (api_key,user_id,expires,key_type) VALUES (?,1,'2999-12-31 23:59:59','default')");$s->execute([$k]);echo "ready";"""
        docker("exec","-i",name,"php","-r",code,input=data["api_key"].encode())
        req=urllib.request.Request(data["url"]+"/api/system/info",headers={"GROCY-API-KEY":data["api_key"]})
        data["version"]=json.load(urllib.request.urlopen(req,timeout=5))["grocy_version"]["Version"]
        manifest.write_text(json.dumps(data))
        print("Disposable fixture ready; Grocy",data["version"])
    except Exception:
        stop(state)
        raise

def stop(state):
    data=load(state)
    docker("info","--format","{{.ServerVersion}}") # Keep the manifest if the daemon is unavailable.
    ident=data["name"].removeprefix("hermes-grocy-contract-")
    for kind,name in [("container",data["name"]),("volume",data["volume"]),("network",data["network"])]:
        try:
            label=docker(kind,"inspect",name,"--format",'{{ index .Labels "'+LABEL+'" }}' if kind!="container" else '{{ index .Config.Labels "'+LABEL+'" }}')
            if label != ident: raise RuntimeError("Refusing to remove a resource not owned by this fixture")
            if kind=="container":docker("rm","-f",name)
            else:docker(kind,"rm",name)
        except subprocess.CalledProcessError: pass
    (state / "fixture.json").unlink()
    try: state.rmdir()
    except OSError: pass # Leave any unrelated files and their directory intact.
    print("Owned disposable fixture removed")

def test(state):
    env=os.environ.copy()
    env["GROCY_CONTRACT_MANIFEST"]=str(state / "fixture.json")
    subprocess.run(["cargo","test","--test","docker_contract","--","--ignored","--nocapture"],env=env,check=True)

if __name__=="__main__":
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("action",choices=["start","test","stop"])
    ap.add_argument("--state",type=pathlib.Path,required=True,help="Dedicated disposable state directory outside the repository")
    args=ap.parse_args()
    globals()[args.action](args.state)
