//! Machine and pack name checks.
//!
//! smoltop's rule, applied before any argument vector is built: a name is
//! non-empty, at most 128 bytes (smolvm's own cap), drawn from `[A-Za-z0-9._-]`,
//! and does not start with `-`. `.` and `..` are rejected so a name cannot be a
//! path-traversal component when it is joined onto the packs directory.

pub const MAX_NAME_LEN: usize = 128;

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name.len() > MAX_NAME_LEN {
        return Err(format!("name is too long (max {MAX_NAME_LEN} bytes)"));
    }
    if name == "." || name == ".." {
        return Err("name must not be '.' or '..'".into());
    }
    if name.starts_with('-') {
        return Err("name must not start with '-'".into());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-'))
    {
        return Err(format!(
            "name contains '{bad}', allowed characters are A-Za-z0-9._-"
        ));
    }
    Ok(())
}

/// Image references are not machine names. Reject whitespace, control
/// characters, and a leading `-` so the value cannot be read as a flag.
pub fn validate_image(image: &str) -> Result<(), String> {
    if image.is_empty() {
        return Err("image is empty".into());
    }
    if image.len() > 512 {
        return Err("image is too long".into());
    }
    if image.starts_with('-') {
        return Err("image must not start with '-'".into());
    }
    if let Some(bad) = image.chars().find(|c| {
        !matches!(
            c,
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' | '/' | ':' | '@'
        )
    }) {
        return Err(format!("image contains '{bad}'"));
    }
    Ok(())
}

pub fn validate_dns(dns: &str) -> Result<String, String> {
    let addr: std::net::Ipv4Addr = dns
        .parse()
        .map_err(|_| format!("dns must be an IPv4 address (got {dns})"))?;
    Ok(addr.to_string())
}

/// First free `tmp-<n>`, starting at 1.
pub fn next_throwaway_name<'a, I>(existing: I) -> Result<String, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let taken: std::collections::HashSet<&str> = existing.into_iter().collect();
    for n in 1..100_000 {
        let candidate = format!("tmp-{n}");
        validate_name(&candidate)?;
        if !taken.contains(candidate.as_str()) {
            return Ok(candidate);
        }
    }
    Err("no free tmp-N name".into())
}

/// Default copy destination: `<source>-copy`, then `<source>-copy-2`, ...
pub fn next_copy_name<'a, I>(source: &str, existing: I) -> Result<String, String>
where
    I: IntoIterator<Item = &'a str>,
{
    validate_name(source)?;
    let taken: std::collections::HashSet<&str> = existing.into_iter().collect();
    let first = format!("{source}-copy");
    if validate_name(&first).is_ok() && !taken.contains(first.as_str()) {
        return Ok(first);
    }
    for n in 2..10_000 {
        let candidate = format!("{source}-copy-{n}");
        validate_name(&candidate)?;
        if !taken.contains(candidate.as_str()) {
            return Ok(candidate);
        }
    }
    Err("no free copy name".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for name in ["web", "tmp-1", "a.b_c", "A1", "n", "machine_01"] {
            assert!(validate_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_unsafe_names() {
        for name in [
            "",
            "-web",
            "--help",
            ".",
            "..",
            "has space",
            "slash/name",
            "back\\slash",
            "semi;",
            "quote'",
            "star*",
            "ü",
            "../etc",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        let long = "a".repeat(MAX_NAME_LEN + 1);
        assert!(validate_name(&long).is_err());
        assert!(validate_name(&"a".repeat(MAX_NAME_LEN)).is_ok());
    }

    #[test]
    fn image_and_dns_rules() {
        assert!(validate_image("alpine").is_ok());
        assert!(validate_image("ghcr.io/org/img:1.2").is_ok());
        assert!(validate_image("reg.example:5000/org/name@sha256").is_ok());
        assert!(validate_image("").is_err());
        assert!(validate_image("-alpine").is_err());
        assert!(validate_image("alpine latest").is_err());
        assert!(validate_image("alpine\nlatest").is_err());
        assert_eq!(validate_dns("100.64.0.7").unwrap(), "100.64.0.7");
        assert!(validate_dns("100.64.0.07").is_err());
        assert!(validate_dns("").is_err());
        assert!(validate_dns("100.64.0.7/32").is_err());
        assert!(validate_dns("::1").is_err());
        assert!(validate_dns("not-an-ip").is_err());
    }

    #[test]
    fn throwaway_and_copy_names_skip_taken_ones() {
        assert_eq!(
            next_throwaway_name(["web", "tmp-1", "tmp-2"]).unwrap(),
            "tmp-3"
        );
        assert_eq!(next_throwaway_name(Vec::<&str>::new()).unwrap(), "tmp-1");
        assert_eq!(next_copy_name("web", ["other"]).unwrap(), "web-copy");
        assert_eq!(next_copy_name("web", ["web-copy"]).unwrap(), "web-copy-2");
        assert!(next_copy_name("-web", Vec::<&str>::new()).is_err());
    }
}
