// Device-free research harness: normal importer and retarget code, no renderer.
use std::{io::{self, BufRead, Write}, path::PathBuf};
use serde_json::{json, Value};
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::GltfImporter;
use un_avatar_skeleton::{HumanoidRetargetContext, ApplyUnMotionFrameOpts, apply_un_motion_frame_to_document_with_context};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(&args[1]);
    let mut ctx = ImportContext { asset_root: path.parent().unwrap().into(),
        enabled_animator_action_ids: vec!["animator:3:1:open1:0".into(),"animator:3:1:gun2:0".into()],
        ..ImportContext::dummy() };
    let imported = GltfImporter.import(&mut ctx, ImportInput::Path(path), ImportOptions).map_err(|e| format!("{e}"))?;
    let mut doc = imported.document;
    let scene = doc.scene.as_ref().ok_or("missing scene")?;
    let rest = scene.nodes.clone();
    let slim_meshes: Vec<Value> = scene.meshes.iter().map(|prims| json!(prims.iter().map(|p| {
        let mut positions=p.positions.clone();
        for (target,weight) in p.morph_targets.iter().zip(&p.default_morph_weights) {
            if weight.abs()>1e-6 { for (v,d) in positions.iter_mut().zip(&target.position_deltas) {
                for k in 0..3 {v[k]+=d[k]*weight;}
            }}
        }
        json!({"name":p.name,"positions":positions,"joints":p.joints,"weights":p.weights,"indices":p.indices,
        "material":p.material_index,"baked_default_morph_weights":p.default_morph_weights})
    }).collect::<Vec<_>>())).collect();
    std::fs::write(&args[2], serde_json::to_vec(&json!({"nodes":scene.nodes,"roots":scene.roots,"skins":scene.skins,
        "constraints":scene.node_constraints,"profile":doc.humanoid_profile,"meshes":slim_meshes,
        "runtime_actions":doc.runtime_actions,"report":imported.report}))?)?;
    drop(slim_meshes);
    let context = HumanoidRetargetContext::for_document(&doc, Some(&rest));
    println!("{}", json!({"ready":true,"node_count":rest.len()}));
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let req: Value = serde_json::from_str(&line?)?;
        if req.get("stop").is_some() { break; }
        let frame = serde_json::from_value(req["frame"].clone())?;
        // Independent pose probes reset by default. Sequence probes explicitly
        // retain preceding transforms, matching the renderer's partial updates.
        if req.get("reset").and_then(Value::as_bool).unwrap_or(true) {
            doc.scene.as_mut().unwrap().nodes.clone_from(&rest);
        }
        apply_un_motion_frame_to_document_with_context(&mut doc, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest), &context);
        let scene = doc.runtime_model().scene().ok_or("missing runtime scene")?;
        let transforms: Vec<_> = if let Some(ids) = req.get("indices").and_then(Value::as_array) {
            ids.iter().map(|i| scene.nodes[i.as_u64().unwrap() as usize].transform).collect()
        } else { scene.nodes.iter().map(|n| n.transform).collect() };
        println!("{}", json!({"transforms":transforms}));
        io::stdout().flush()?;
    }
    Ok(())
}
