use fluxdown_protocol::{
    AgentLoginResult, AgentSessionDto, CloudDevice, CloudUser, CloudUserStatus, Entitlements,
    GatewayPatchParams, RemoteTaskStatus,
};
use serde_json::json;

fn session() -> AgentSessionDto {
    AgentSessionDto {
        user: CloudUser {
            id: "user-1".to_owned(),
            email: "user@example.com".to_owned(),
            nickname: "User".to_owned(),
            plan: "pro".to_owned(),
            status: CloudUserStatus::Active,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            last_login_at: None,
            origin_id: Some(10001),
            origin_id_changed: false,
            membership_ordinal: Some(7),
        },
        entitlements: Entitlements::default(),
        current_plan: None,
        device: CloudDevice {
            id: "row-1".to_owned(),
            device_id: "device-1".to_owned(),
            name: "Desktop".to_owned(),
            platform: Some("linux".to_owned()),
            created_at: String::new(),
            last_seen_at: String::new(),
            last_ip: None,
            app_version: Some("1.0.0".to_owned()),
            is_online: true,
            is_current: true,
        },
    }
}

#[test]
fn local_auth_results_never_serialize_cloud_tokens() -> Result<(), serde_json::Error> {
    let wire = serde_json::to_value(AgentLoginResult::Ok {
        session: Box::new(session()),
    })?;
    let text = serde_json::to_string(&wire)?;
    assert_eq!(wire["status"], "ok");
    assert!(!text.contains("accessToken"));
    assert!(!text.contains("refreshToken"));
    assert!(!text.to_ascii_lowercase().contains("bearer"));
    Ok(())
}

#[test]
fn verification_metadata_uses_camel_case_without_tokens() -> Result<(), serde_json::Error> {
    let wire = serde_json::to_value(AgentLoginResult::DeviceVerificationRequired {
        ttl_seconds: 300,
        will_replace_devices: true,
    })?;
    assert_eq!(
        wire,
        json!({
            "status": "deviceVerificationRequired",
            "ttlSeconds": 300,
            "willReplaceDevices": true
        })
    );
    Ok(())
}

#[test]
fn entitlement_unknown_fields_roundtrip_losslessly() -> Result<(), serde_json::Error> {
    let input = json!({
        "maxSyncDevices": 5,
        "futureCapability": {"nested": [1, true, "x"]}
    });
    let entitlements = serde_json::from_value::<Entitlements>(input.clone())?;
    assert_eq!(serde_json::to_value(entitlements)?, input);
    Ok(())
}

#[test]
fn unknown_remote_status_falls_back_to_pending() -> Result<(), serde_json::Error> {
    let status = serde_json::from_value::<RemoteTaskStatus>(json!("futureState"))?;
    assert_eq!(status, RemoteTaskStatus::Pending);
    Ok(())
}

#[test]
fn gateway_patch_is_partial_and_never_echoes_user_token() -> Result<(), serde_json::Error> {
    let patch = serde_json::from_value::<GatewayPatchParams>(json!({
        "corsEnabled": true,
        "userToken": "secret"
    }))?;
    assert_eq!(patch.cors_enabled, Some(true));
    assert_eq!(patch.api_enabled, None);
    let status = fluxdown_protocol::GatewayStatusDto {
        cors_enabled: true,
        user_token_configured: true,
        ..Default::default()
    };
    let wire = serde_json::to_string(&status)?;
    assert!(wire.contains("\"userTokenConfigured\":true"));
    assert!(!wire.contains("secret"));
    Ok(())
}
