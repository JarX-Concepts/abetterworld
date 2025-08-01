use std::{sync::Arc, thread, time::Duration};

use crate::{
    camera::Camera,
    content::{Tile, TileState},
    errors::AbwError,
};
use cgmath::MetricSpace;
use crossbeam_channel::{Receiver, Sender};

pub fn prioritize(
    backlog: &mut Vec<Tile>,
    last_cam_gen: &mut u64,
    pager_rx: &Receiver<Tile>,
    loader_tx: &Sender<Tile>,
    cam: &Arc<Camera>,
) -> Result<bool, AbwError> {
    let mut did_nothing_iter = true;

    if backlog.is_empty() {
        //thread::sleep(Duration::from_secs(5));

        // No backlog, block wait for a tile
        let t = pager_rx.recv();
        if let Ok(tile) = t {
            backlog.push(tile);
            did_nothing_iter = false;
        }
    }

    // ingest new tiles -------------------------------------------------
    for t in pager_rx.try_iter() {
        if t.state == TileState::ToLoad {
            backlog.push(t);
            did_nothing_iter = false;
        }
    }

    // detect camera movement ------------------------------------------
    let new_gen = cam.generation();
    if new_gen != *last_cam_gen || did_nothing_iter == false {
        let camera_data = cam.refinement_data();
        backlog.sort_unstable_by(|a, b| {
            let da = camera_data.position.distance2(a.volume.center());
            let db = camera_data.position.distance2(b.volume.center());
            db.partial_cmp(&da).unwrap()
        });
        *last_cam_gen = new_gen;
    }

    // feed workers -----------------------------------------------------
    while let Some(tile) = backlog.last() {
        // ≈ cheapest (small dist) at back
        if loader_tx.try_send(tile.clone()).is_ok() {
            backlog.pop();
            did_nothing_iter = false;
        } else {
            break;
        }
    }

    Ok(did_nothing_iter)
}

pub fn priortize_loop(cam: &Arc<Camera>, pager_rx: &Receiver<Tile>, loader_tx: &Sender<Tile>) {
    let mut backlog: Vec<Tile> = Vec::new();
    let mut last_cam_gen = 0;

    loop {
        let did_something =
            prioritize(&mut backlog, &mut last_cam_gen, &pager_rx, &loader_tx, &cam)
                .map_err(|e| {
                    log::error!("prioritization failed: {e}");
                })
                .unwrap_or_else(|_| false);

        if did_something {
            // No new tiles or camera movement, sleep briefly to avoid busy-waiting
            thread::sleep(Duration::from_millis(10));
        }
    }
}
