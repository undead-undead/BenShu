use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use reqwest::Url;

#[derive(Debug, Clone)]
pub(crate) struct PublicUrlParts {
    pub(crate) host: String,
}

pub(crate) fn validate_public_http_url(raw_url: &str) -> Result<PublicUrlParts, String> {
    let parsed = Url::parse(raw_url).map_err(|_| "URL must be absolute and valid".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported URL scheme '{}'; web tools only allow http/https",
            parsed.scheme()
        ));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not embed credentials".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?
        .trim()
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err("URL host is empty".to_string());
    }
    if host_is_local_or_private(&host) {
        return Err(format!(
            "host '{host}' is local, private, or reserved; web tools only open public web URLs"
        ));
    }

    Ok(PublicUrlParts { host })
}

pub(crate) fn host_looks_public(host: &str) -> bool {
    let lowered = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if lowered.is_empty() || lowered.parse::<IpAddr>().is_ok() {
        return false;
    }
    !host_is_local_name(&lowered) && lowered.contains('.')
}

pub(crate) fn ip_targets_internal_resource(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_is_local_or_private(ip),
        IpAddr::V6(ip) => ipv6_is_local_or_private(ip),
    }
}

pub(crate) fn ip_is_fake_proxy_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 198 && matches!(octets[1], 18 | 19)
        }
        IpAddr::V6(v6) => v6.is_unique_local(),
    }
}

fn host_is_local_or_private(host: &str) -> bool {
    if host_is_local_name(host) {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_targets_internal_resource(ip);
    }

    false
}

fn host_is_local_name(host: &str) -> bool {
    matches!(host, "localhost" | "localhost.localdomain")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host.ends_with(".home")
        || host.ends_with(".home.arpa")
}

fn ipv4_is_local_or_private(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        || ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn ipv6_is_local_or_private(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_is_local_or_private(mapped);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use super::validate_public_http_url;

    #[test]
    fn public_url_validation_blocks_private_targets() {
        for url in [
            "http://localhost:3000",
            "http://127.0.0.1:9222/json/version",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "file:///etc/passwd",
        ] {
            assert!(validate_public_http_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn public_url_validation_allows_public_https() {
        assert!(validate_public_http_url("https://example.com/path").is_ok());
    }
}
