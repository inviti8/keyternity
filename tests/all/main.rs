//! Keyternity's integration tests, as one test binary: one link of the app instead of
//! one per file, which keeps memory and build time down. Run them all with
//! `cargo test --test all`, or some by module path, e.g.
//! `cargo test --test all -- topology:: pen::`.

mod agent;
mod anim_end;
mod audio;
mod auto_mesh;
mod bind_pose;
mod bone_length;
mod camera_ui;
mod crash_log;
mod delete_bones;
mod dragonbones_export;
mod dragonbones_import;
mod envelope;
mod envelope_tool;
mod export_sequence;
mod face;
mod face_gen;
mod face_panel_frames;
mod ffmpeg_download;
mod ik;
mod ik_space;
mod keyframes;
mod layer_events;
mod layer_loops;
mod layers;
mod lipsync;
mod lipsync_langs;
mod mesh_tools;
mod navigation;
mod palette;
mod pen;
mod psd_import;
mod regions;
mod render_camera;
mod shapes;
mod skin;
mod tex_sheet;
mod timeline_edit;
mod timeline_rows;
mod timeline_view;
mod topology;
mod transcribe;
mod visibility;
mod weights;
