// Opt-in cross-process transport probe: no UI, renderer, audio, or user settings.
use super::*;
use zenoh::Wait as _;

#[test]
#[ignore = "explicit bounded transport diagnostic only"]
fn peer() {
    let key = std::env::var("UN_AVATAR_PROBE_KEY").expect("probe key required");
    let publisher = zenoh::open(peer_config()).wait().unwrap();
    eprintln!("peer status session ready: {key}");
    let control = zenoh::open(peer_config()).wait().unwrap();
    let sub = control.declare_subscriber(format!("{key}/control")).wait().unwrap();
    eprintln!("peer control subscribed: {key}");
    let until = Instant::now() + Duration::from_secs(90);
    let mut published = false;
    while Instant::now() < until {
        let result = publisher.put(format!("{key}/status"), r#"{"connected":true,"protocol":"zenoh-json-v1","control_capabilities":["shutdown"],"uptime_secs":9,"fps":60.0,"cpu_ms":1.0,"gpu_ms":2.0,"ram_mb":null,"surface_width":640,"surface_height":360,"aa":"msaa","texture_resolution_limit":"off","texture_compression":"source","processed_texture_cache":true,"texture_summary":{"image_count":1,"uploaded_mip_bytes":512},"spout_available":true,"spout_enabled":false,"spout_name":null,"spout_width":null,"spout_height":null,"dynamics_response_categories":[{"category":"hair","group_count":2,"joint_count":5,"xpbd_group_count":1,"average_rest_response":0.2,"min_rest_response":0.1,"max_rest_response":0.3,"average_pull":0.2,"average_stiffness":0.3,"average_shape_preservation":0.25,"min_shape_preservation":0.12,"max_shape_preservation":0.31,"average_bounce_response":0.4,"min_bounce_response":0.2,"max_bounce_response":0.5,"average_spring":0.4,"average_drag_force":0.5,"average_damping_half_life_ms":140.0,"average_parent_motion_follow":0.6,"min_parent_motion_follow":0.4,"max_parent_motion_follow":0.7,"average_orientation_follow":0.15,"average_xpbd_compliance":0.01}],"dynamics_response_groups":[{"source_id":"physbone:hair","category":"hair","joint_count":5,"solver":"xpbd","average_rest_response":0.2,"min_rest_response":0.1,"max_rest_response":0.3,"average_pull":0.2,"average_stiffness":0.3,"average_shape_preservation":0.25,"min_shape_preservation":0.12,"max_shape_preservation":0.31,"average_bounce_response":0.4,"min_bounce_response":0.2,"max_bounce_response":0.5,"average_spring":0.4,"average_drag_force":0.5,"average_damping_half_life_ms":140.0,"average_parent_motion_follow":0.6,"min_parent_motion_follow":0.4,"max_parent_motion_follow":0.7,"average_orientation_follow":0.15,"xpbd_compliance":0.01}],"note":null}"#).wait();
        if !published { eprintln!("peer first publish: {key}: {result:?}"); published = true; }
        if let Some(sample) = sub.recv_timeout(Duration::from_millis(250)).unwrap() {
            let request: serde_json::Value = serde_json::from_slice(&sample.payload().to_bytes()).unwrap();
            let id = request["request_id"].as_str().unwrap();
            eprintln!("peer received control: {key}: {id}");
            control.put(format!("{key}/control/response/{id}"), serde_json::json!({"request_id":id,"ok":true}).to_string()).wait().unwrap();
        }
    }
}

struct Children(Vec<std::process::Child>);

#[test]
#[ignore = "explicit bounded transport diagnostic only"]
fn no_subscriber() {
    let session=zenoh::open(zenoh::Config::default()).wait().unwrap();
    let key=format!("un-avatar/probe/absent/{}/{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let started=Instant::now();
    let error=publish_ready_runtime_control(&session,&key,"{}".to_string()).unwrap_err();
    assert!(error.contains("request not sent"),"{error}");
    assert!(started.elapsed()<Duration::from_secs(12));
    eprintln!("probe absent subscriber rejected without send: {error}");
}

impl Drop for Children {
    fn drop(&mut self) { for child in &mut self.0 { let _=child.kill(); let _=child.wait(); } }
}
#[test]
#[ignore = "explicit bounded transport diagnostic only"]
fn three_peers() {
    let mut children=Children(Vec::new());
    let base=format!("un-avatar/probe/{}/{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let mut streams=Vec::new();
    let start=Instant::now();
    for id in 1..=3 {
        let key=format!("{base}/{id}");
        children.0.push(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["runtime_bus_probe::peer","--exact","--ignored","--nocapture"])
            .env("UN_AVATAR_PROBE_KEY",&key).spawn().unwrap());
        let (cache,stop)=spawn_runtime_status_stream(key.clone(),id,String::new());
        streams.push((key,cache,stop));
    }
    let mut received=[false;3];
    while start.elapsed()<Duration::from_secs(45) && received.iter().any(|v| !v) {
        for (i,(key,cache,_)) in streams.iter().enumerate() {
            if !received[i] && cache.lock().unwrap().telemetry.is_some() {
                received[i]=true; eprintln!("probe first telemetry: {key} at {:?}",start.elapsed());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut controls=Vec::new();
    for (key,_,_) in &streams {
        let result=send_renderer_control_bus(key,&RendererControlCommand::ResetCamera);
        eprintln!("probe first control: {key}: {result:?}"); controls.push(result);
    }
    for (_,_,stop) in &streams {stop.store(true,Ordering::Release);}
    assert!(received.iter().all(|v| *v),"telemetry missing: {received:?}");
    assert!(controls.iter().all(Result::is_ok),"control failure: {controls:?}");
}


fn peer_config() -> zenoh::Config {
    let mut config=zenoh::Config::default();
    if let Ok(endpoint)=std::env::var("UN_AVATAR_PROBE_ENDPOINT") {
        config.insert_json5("mode", "\"client\"").unwrap();
        config.insert_json5("connect/endpoints", &serde_json::json!([endpoint]).to_string()).unwrap();
        config.insert_json5("scouting/multicast/enabled", "false").unwrap();
        config.insert_json5("scouting/gossip/enabled", "false").unwrap();
    }
    config
}

#[test]
#[ignore = "explicit bounded transport diagnostic only"]
fn loopback_three_peers() {
    let probe=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint=format!("tcp/{}",probe.local_addr().unwrap());
    drop(probe); // Diagnostic only; Zenoh bind failure aborts if the port is claimed.
    let mut config=zenoh::Config::default();
    config.insert_json5("mode", "\"router\"").unwrap();
    config.insert_json5("listen/endpoints", &serde_json::json!([endpoint]).to_string()).unwrap();
    config.insert_json5("scouting/multicast/enabled", "false").unwrap();
    config.insert_json5("scouting/gossip/enabled", "false").unwrap();
    let session=zenoh::open(config).wait().unwrap();
    assert!(endpoint.starts_with("tcp/127.0.0.1:"));
    shared_session_probe(session, Some(&endpoint));
}

#[test]
#[ignore = "explicit bounded transport diagnostic only"]
fn shared_default_three_peers() {
    shared_session_probe(zenoh::open(zenoh::Config::default()).wait().unwrap(), None);
}

fn shared_session_probe(session: zenoh::Session, endpoint: Option<&str>) {
    let base=format!("un-avatar/probe/local/{}/{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let sub=session.declare_subscriber(format!("{base}/*/status")).wait().unwrap();
    let mut children=Children(Vec::new());
    let start=Instant::now();
    for id in 1..=3 {
        let mut command=std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["runtime_bus_probe::peer","--exact","--ignored","--nocapture"])
            .env("UN_AVATAR_PROBE_KEY",format!("{base}/{id}"))
            .env_remove("UN_AVATAR_PROBE_ENDPOINT");
        if let Some(endpoint)=endpoint { command.env("UN_AVATAR_PROBE_ENDPOINT",endpoint); }
        children.0.push(command.spawn().unwrap());
    }
    let mut received=std::collections::BTreeSet::new();
    while start.elapsed()<Duration::from_secs(45) && received.len()<3 {
        if let Some(sample)=sub.recv_timeout(Duration::from_millis(100)).unwrap() {
            let state:RendererRuntimeTelemetry=serde_json::from_slice(&sample.payload().to_bytes()).unwrap();
            assert!(state.connected);
            if received.insert(sample.key_expr().to_string()) {
                eprintln!("shared-session first telemetry: {} at {:?}",sample.key_expr(),start.elapsed());
            }
        }
    }
    assert_eq!(received.len(),3,"Missing loopback telemetry");
    for id in 1..=3 {
        let response_key=format!("{base}/{id}/control/response/check-{id}");
        let response_sub=session.declare_subscriber(&response_key).wait().unwrap();
        publish_ready_runtime_control(&session,&format!("{base}/{id}/control"),serde_json::json!({"request_id":format!("check-{id}"),"command":{"command":"reset_camera"}}).to_string()).unwrap();
        let response=response_sub.recv_timeout(Duration::from_secs(10)).unwrap().expect("response missing");
        let response:RuntimeBusControlResponse=serde_json::from_slice(&response.payload().to_bytes()).unwrap();
        assert_eq!(response.request_id,format!("check-{id}")); assert!(response.ok);
    }
    eprintln!("shared-session all telemetry and first controls done at {:?}",start.elapsed());
}
