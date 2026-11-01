use materialcraft_engine::{Session, command_specs, execute, sample_session};
use serde_json::{Value, json};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    execute(s, id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

#[test]
fn every_template_cooks_cleanly() {
    let mut s = sample_session();
    s.preview_size = Some(128);
    for (id, _) in materialcraft_engine::samples::TEMPLATES {
        run(&mut s, "file.new", json!({"template": id, "size": 256}));
        let c = s.cook().clone();
        let g = s.graph().unwrap();
        for (nid, r) in &c.nodes {
            assert!(r.error.is_none(), "{id}: node {} ({}) failed: {:?}", nid.0, g.node(*nid).unwrap().def, r.error);
        }
        if *id != "empty" {
            assert!(!s.material_maps().is_empty(), "{id} has no outputs");
        }
    }
}

#[test]
fn build_a_graph_by_commands() {
    let mut s = sample_session();
    run(&mut s, "file.new", json!({"template": "empty", "name": "Mine"}));
    let noise = run(&mut s, "graph.node.add", json!({"def": "clouds_1", "at": [0, 0], "params": {"scale": 6}}))["id"].as_u64().unwrap();
    let lv = run(&mut s, "graph.node.add", json!({"def": "levels", "connectFrom": {"node": noise}}))["id"].as_u64().unwrap();
    let out = run(&mut s, "graph.node.add", json!({"def": "output", "at": [400, 0], "params": {"usage": 4}}))["id"].as_u64().unwrap();
    run(&mut s, "graph.connect", json!({"from": lv, "to": out}));
    run(&mut s, "graph.node.param", json!({"id": lv, "param": "in_low", "value": 0.3}));
    run(&mut s, "graph.node.param", json!({"id": lv, "param": "in_low", "value": 0.4}));
    // A slider drag is one undo step.
    assert_eq!(s.history().iter().filter(|h| *h == "Set parameter").count(), 1);
    s.cook();
    let i = run(&mut s, "session.inspect", json!({}));
    let nodes = i["graph"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().all(|n| n["cooked"]["error"].is_null()));
    assert_eq!(i["graph"]["connections"].as_array().unwrap().len(), 2);
    // Bad input is an error, not a panic, and leaves the graph unchanged.
    assert!(execute(&mut s, "graph.connect", &json!({"from": out, "to": noise})).is_err());
    assert!(execute(&mut s, "graph.node.param", &json!({"id": lv, "param": "nope", "value": 1})).is_err());
    assert!(execute(&mut s, "graph.node.add", &json!({"def": "nope"})).is_err());
    // Undo and redo.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    // Copy/paste and duplicate.
    run(&mut s, "graph.select", json!({"ids": [noise, lv]}));
    run(&mut s, "edit.copy", json!({}));
    run(&mut s, "edit.paste", json!({"at": [0, 300]}));
    assert_eq!(s.graph().unwrap().nodes.len(), 5);
    assert_eq!(s.graph().unwrap().connections.len(), 3);
    run(&mut s, "edit.duplicate", json!({}));
    assert_eq!(s.graph().unwrap().nodes.len(), 7);
    // Create a graph from a selection: the nodes move into a sub-graph instance.
    run(&mut s, "graph.select", json!({"ids": [lv]}));
    let r = run(&mut s, "graph.createFromSelection", json!({"name": "Leveler"}));
    assert_eq!(r["graph"], "Leveler");
    s.cook();
    assert!(s.cooked.nodes.values().all(|r| r.error.is_none()));
    // Exposed inputs drive bound parameters.
    run(&mut s, "graph.open", json!({"graph": "Mine"}));
    run(&mut s, "graph.exposed.add", json!({"id": "detail", "bind": {"node": noise, "param": "scale"}}));
    run(&mut s, "graph.exposed.set", json!({"id": "detail", "value": 12}));
    s.cook();
    assert!(s.cooked.nodes.values().all(|r| r.error.is_none()));
}

#[test]
fn package_save_and_open_round_trip() {
    let mut s = sample_session();
    let dir = std::env::temp_dir().join(format!("mc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bricks.mcraft");
    run(&mut s, "file.save", json!({"path": path.to_str().unwrap()}));
    let before = s.pkg.clone();
    run(&mut s, "file.newPackage", json!({}));
    run(&mut s, "file.open", json!({"path": path.to_str().unwrap()}));
    assert_eq!(s.pkg.graphs, before.graphs);
    let out = dir.join("tex");
    let r = run(&mut s, "file.export", json!({"dir": out.to_str().unwrap(), "format": "png", "depth": 8}));
    assert_eq!(r["files"].as_array().unwrap().len(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every command, run with empty and hostile params, returns Ok or Err but never panics.
#[test]
fn panic_hunt() {
    let hostile = [
        json!({}),
        json!(null),
        json!({"id": u64::MAX, "ids": [u64::MAX, 0], "node": -1, "value": "x", "param": "", "def": "", "graph": 999}),
        json!({"width": 0, "height": u64::MAX, "size": -5, "seed": f64::MAX, "at": [f64::MAX, -1e30], "by": ["a"], "rect": [1, 2]}),
        json!({"value": [1e38, -1e38, 0, 0], "param": "opacity", "def": "blend", "mode": "zzz", "format": "gif", "dir": "", "path": ""}),
        json!({"from": 0, "to": 0, "fromPort": "", "toPort": "", "clip": {"nodes": "x"}, "template": "../../x", "scene": "\u{0}"}),
    ];
    for spec in command_specs() {
        // Skip file writes to the working directory and opening paths.
        if matches!(spec.id, "file.save" | "file.saveAs" | "file.export" | "file.open" | "file.importBitmap") {
            continue;
        }
        for p in &hostile {
            let mut s = sample_session();
            s.preview_size = Some(64);
            let first = s.graph().unwrap().nodes[0].id;
            s.selection = vec![first];
            let _ = execute(&mut s, spec.id, p);
            s.cook();
        }
    }
}
