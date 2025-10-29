use std::any;

use crate::{
    content::{
        ChildrenKeys, RefineMode, TileInfo, TileManager, TileSource, TileSourceContent,
        TileSourceContentState,
    },
    dynamics::CameraRefinementData,
    helpers::{is_bounding_volume_visible, AbwError},
};
use cgmath::MetricSpace;

pub struct Pri<'a> {
    pub tile: &'a TileSource,
    pub tile_content: &'a TileSourceContent,
    pub tile_info: Option<TileInfo>,
    pub parent_visual_id: Option<u64>,
    pub priority: f64,
}

pub struct PriorityTiles<'a> {
    pub inview: Vec<Pri<'a>>,
    pub outofview: Vec<Pri<'a>>,
    pub still_loading: Vec<Pri<'a>>,
}

pub fn gather_priority_tiles<'a>(
    tile_manager: &TileManager,
    camera_data: &CameraRefinementData,
    tile: &'a TileSource,
    out: &mut PriorityTiles<'a>,
    parent_visual_id: Option<u64>,
) -> Result<(), AbwError> {
    let mut current_parent_visual_id = parent_visual_id;
    let mut found_visual_tile = None;

    if let Some(refinement_stage) = tile.needs_refinement_flag {
        if let Some(content) = &tile.content {
            match &content.loaded {
                Some(TileSourceContentState::Visual) => {
                    let distance = camera_data
                        .position
                        .distance2(tile.bounding_volume.center());

                    let priority_tile = Pri {
                        tile,
                        tile_content: content,
                        tile_info: None,
                        parent_visual_id: current_parent_visual_id,
                        priority: distance,
                    };
                    current_parent_visual_id = Some(content.key);
                    found_visual_tile = Some(priority_tile);
                }
                Some(TileSourceContentState::LoadingTileSet { .. }) => {
                    out.still_loading.push(Pri {
                        tile,
                        tile_content: content,
                        tile_info: None,
                        parent_visual_id: current_parent_visual_id,
                        priority: f64::MAX, // lowest priority
                    });
                }
                Some(TileSourceContentState::LoadedTileSet { permanent }) => {
                    if let Some(permanent_root) = permanent {
                        if let Some(root) = &permanent_root.root {
                            gather_priority_tiles(
                                tile_manager,
                                camera_data,
                                root,
                                out,
                                current_parent_visual_id,
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }

        if refinement_stage == true {
            if let Some(children) = &tile.children {
                for child in children {
                    gather_priority_tiles(
                        tile_manager,
                        camera_data,
                        child,
                        out,
                        current_parent_visual_id,
                    )?;
                }
            }
        }

        if let Some(found_visual_tile) = &mut found_visual_tile {
            let parent_key = found_visual_tile.tile_content.key;

            let any_children_loading = out
                .still_loading
                .iter()
                .any(|p| p.parent_visual_id == Some(parent_key));

            let children_opt = if !any_children_loading {
                let children: ChildrenKeys = out
                    .inview
                    .iter()
                    .chain(out.outofview.iter())
                    .filter_map(|p| {
                        (p.parent_visual_id == Some(parent_key)).then_some(p.tile_content.key)
                    })
                    .collect();

                if children.is_empty() {
                    None
                } else {
                    Some(children)
                }
            } else {
                None
            };

            found_visual_tile.tile_info = Some(TileInfo {
                children: children_opt,
                parent: found_visual_tile.parent_visual_id,
                volume: tile.bounding_volume.clone(),
                refine: RefineMode::Replace, //tile.refine,
                geometric_error: tile.geometric_error,
            });
        }

        // borrow ends here; now take ownership
        if let Some(found_visual_tile) = found_visual_tile.take() {
            if is_bounding_volume_visible(&camera_data.planes, &tile.bounding_volume.to_aabb()) {
                out.inview.push(found_visual_tile);
            } else {
                out.outofview.push(found_visual_tile);
            }
        }
    }

    Ok(())
}

pub fn priortize<'a>(
    tile_manager: &TileManager,
    camera_data: &CameraRefinementData,
    tile: &'a TileSource,
    out_tiles: &mut Vec<Pri<'a>>,
) -> Result<(), AbwError> {
    let mut out: PriorityTiles<'a> = PriorityTiles {
        inview: Vec::new(),
        outofview: Vec::new(),
        still_loading: Vec::new(),
    };
    gather_priority_tiles(tile_manager, camera_data, tile, &mut out, None)?;

    // sort by priority (distance)
    out.inview
        .sort_unstable_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
    out.outofview
        .sort_unstable_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
    out_tiles.extend(out.inview);
    out_tiles.extend(out.outofview);

    Ok(())
}
