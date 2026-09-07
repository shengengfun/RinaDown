use fluxdown_protocol::ApplicationErrorCode;
use serde_json::json;

#[test]
fn application_error_literals_are_exact() -> Result<(), serde_json::Error> {
    let cases = [
        (
            ApplicationErrorCode::ProtocolIncompatible,
            "protocolIncompatible",
        ),
        (ApplicationErrorCode::Unauthorized, "unauthorized"),
        (ApplicationErrorCode::InvalidArgument, "invalidArgument"),
        (ApplicationErrorCode::NotFound, "notFound"),
        (ApplicationErrorCode::Conflict, "conflict"),
        (ApplicationErrorCode::Unavailable, "unavailable"),
        (ApplicationErrorCode::Timeout, "timeout"),
        (ApplicationErrorCode::Cancelled, "cancelled"),
        (ApplicationErrorCode::Unsupported, "unsupported"),
        (ApplicationErrorCode::Internal, "internal"),
    ];
    for (code, literal) in cases {
        assert_eq!(serde_json::to_value(code)?, json!(literal));
    }
    Ok(())
}
