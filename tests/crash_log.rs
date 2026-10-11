//! The crash log must land somewhere writable and easy to find: the app data
//! folder, beside config.json (not the install folder, which isn't writable).

use keyternity_lib::{shared, utils};

#[test]
fn crash_log_sits_beside_config() {
    let log = utils::crashlog_file();
    assert_eq!(log.file_name().unwrap(), "crash.log");
    assert_eq!(log.parent(), shared::config_path().parent());
}
