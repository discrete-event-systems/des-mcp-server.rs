//! Shared, version-neutral startup policy pinned to `mcp-rust-libs`.

use ore_mcp_bootstrap::runtime::{IdentityError, ServerIdentity};

pub const SERVICE_NAME: &str = "des-mcp-server";
pub const SERVICE_NAMESPACE: &str = "discrete-event-systems";

pub fn stdio_identity() -> Result<ServerIdentity, IdentityError> {
    ServerIdentity::stdio(SERVICE_NAME, SERVICE_NAMESPACE)
}

#[must_use]
pub fn environment_resource_attributes() -> Vec<(String, String)> {
    ore_mcp_bootstrap::telemetry::environment_resource_attributes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_preserves_the_existing_des_contract() {
        let identity = stdio_identity().expect("valid canonical identity");
        assert_eq!(identity.service_name(), SERVICE_NAME);
        assert_eq!(identity.service_namespace(), SERVICE_NAMESPACE);
        assert_eq!(identity.transport(), "stdio");
    }

    #[test]
    fn shared_attributes_reject_secrets_and_identity_spoofing() {
        assert_eq!(
            ore_mcp_bootstrap::telemetry::resource_attribute_pairs(
                "domain=discrete-event-simulation,password=secret,service.name=spoof,cloud.region=us-east-1",
            ),
            vec![
                ("domain".to_owned(), "discrete-event-simulation".to_owned()),
                ("cloud.region".to_owned(), "us-east-1".to_owned()),
            ]
        );
    }
}
