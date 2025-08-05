use std::{sync::Arc, thread, time::Duration};

use crate::{
    content::types::{Tile, TileState},
    helpers::{
        channel::{Receiver, Sender},
        AbwError,
    },
    render::Camera,
};
use cgmath::MetricSpace;

pub async fn prioritize(
    backlog: &mut Vec<Tile>,
    last_cam_gen: &mut u64,
    pager_rx: &mut Receiver<Tile>,
    loader_tx: &mut Sender<Tile>,
    cam: &Arc<Camera>,
) -> Result<bool, AbwError> {
    let mut did_nothing_iter = true;

    //if backlog.is_empty()
    {
        //thread::sleep(Duration::from_secs(5));

        log::info!("Waiting for new tiles from pager...");

        // No backlog, block wait for a tile
        let tile = pager_rx
            .recv()
            .await
            .map_err(|e| AbwError::Paging(format!("Pager closed: {:?}", e)))?;

        //log::info!("Received tile from pager: {:?}", tile);
        backlog.push(tile);
        did_nothing_iter = false;
    }

    // ingest new tiles -------------------------------------------------
    while let Ok(Some(tile)) = pager_rx.try_next() {
        if tile.state == TileState::ToLoad {
            backlog.push(tile);
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
        log::info!("Send a priority tile");
        if loader_tx.try_send(tile.clone()).is_ok() {
            backlog.pop();
            did_nothing_iter = false;
        } else {
            break;
        }
    }
    Ok(did_nothing_iter)
}

pub async fn priortize_loop(
    cam: &Arc<Camera>,
    pager_rx: &mut Receiver<Tile>,
    loader_tx: &mut Sender<Tile>,
    enable_sleep: bool,
) -> Result<(), AbwError> {
    let mut backlog: Vec<Tile> = Vec::new();
    let mut last_cam_gen = 0;

    loop {
        let did_something =
            prioritize(&mut backlog, &mut last_cam_gen, pager_rx, loader_tx, &cam).await?;

        if did_something {
            // No new tiles or camera movement, sleep briefly to avoid busy-waiting
            if enable_sleep {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
