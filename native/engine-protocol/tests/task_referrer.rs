//! `TaskDto` 必须在 wire 上保留任务来源页。

use fluxdown_engine::model::TaskInfo;

#[test]
fn task_dto_json_carries_referrer() -> Result<(), serde_json::Error> {
    let info = TaskInfo {
        task_id: "t1".to_owned(),
        url: "https://example.com/f.zip".to_owned(),
        file_name: "f.zip".to_owned(),
        save_dir: "/tmp".to_owned(),
        status: 2,
        downloaded_bytes: 10,
        total_bytes: 100,
        error_message: String::new(),
        created_at: "1700000000".to_owned(),
        proxy_url: String::new(),
        queue_id: String::new(),
        checksum: String::new(),
        ignore_tls_errors: false,
        file_missing: false,
        completed_at: String::new(),
        segments: 0,
        queue_order: 0,
        uploaded_bytes: 0,
        uploaded_at_completion: 0,
        seeding_status: 0,
        seeding_message: String::new(),
        seeding_time_secs: 0,
        seed_ratio_limit_milli: -2,
        seed_post_ratio_limit_milli: -2,
        seed_time_limit_minutes: -2,
        seed_inactive_time_limit_minutes: -2,
        seed_upload_limit_bps: 0,
        referrer: "https://example.com/page".to_owned(),
        group_id: String::new(),
        rss_source_id: String::new(),
        origin_url: String::new(),
        auto_route: String::new(),
    };
    let dto = fluxdown_engine_protocol::task_info_to_dto(info);
    assert_eq!(dto.referrer, "https://example.com/page");
    let json = serde_json::to_string(&dto)?;
    assert!(json.contains(r#""referrer":"https://example.com/page""#));
    Ok(())
}
