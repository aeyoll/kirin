use rand::Rng;

pub fn valid_link_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn gen_link_id(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

pub fn gen_delete_code(len: usize) -> String {
    gen_link_id(len)
}

pub fn gen_rolling_code(len: usize) -> String {
    gen_link_id(len)
}

pub fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.2} KiB", bytes as f64 / 1024.0);
    }
    if bytes < 1024 * 1024 * 1024 {
        return format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0));
    }
    format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

pub fn content_disposition(name: &str, inline: bool) -> axum::http::HeaderValue {
    let safe = sanitize_filename::sanitize(name);
    let kind = if inline { "inline" } else { "attachment" };
    let val = if safe.bytes().all(|b| b < 128 && b != b'"' && b != b'\\') {
        format!("{kind}; filename=\"{safe}\"")
    } else {
        let encoded: String = safe
            .chars()
            .map(|c| {
                let mut u = [0u8; 4];
                let s = c.encode_utf8(&mut u);
                s.bytes().fold(String::new(), |mut acc, b| {
                    acc.push_str(&percent_encoding(b));
                    acc
                })
            })
            .collect();
        format!("{kind}; filename=\"download.bin\"; filename*=UTF-8''{encoded}")
    };
    axum::http::HeaderValue::from_str(&val).unwrap_or_else(|_| {
        axum::http::HeaderValue::from_static(if inline {
            "inline; filename=\"file\""
        } else {
            "attachment; filename=\"file\""
        })
    })
}

fn percent_encoding(b: u8) -> String {
    match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
            (b as char).to_string()
        }
        _ => format!("%{:02X}", b),
    }
}

fn ip_matches_entry(entry: &str, client_ip: &str) -> bool {
    if entry.contains('/') {
        if let Ok(net) = entry.parse::<ipnet::Ipv4Net>() {
            if let Ok(addr) = client_ip.parse::<std::net::Ipv4Addr>() {
                return net.contains(&addr);
            }
        }
        false
    } else {
        entry == client_ip
    }
}

fn ip_in_list(list: &[String], client_ip: &str) -> bool {
    list.iter().any(|e| ip_matches_entry(e, client_ip))
}

/// Mirrors `jirafeau_challenge_upload`: nopassword IP, or open upload, or password plus IP rules.
pub fn challenge_upload(
    upload_passwords: &[String],
    upload_ip: &[String],
    upload_ip_nopassword: &[String],
    client_ip: &str,
    supplied_password: Option<&str>,
) -> bool {
    if ip_in_list(upload_ip_nopassword, client_ip) {
        return true;
    }
    let has_pw = !upload_passwords.is_empty();
    let ip_restricted = !upload_ip.is_empty();
    if !has_pw && !ip_restricted {
        return true;
    }
    if !has_pw && ip_restricted {
        return ip_in_list(upload_ip, client_ip);
    }
    let Some(pw) = supplied_password else {
        return false;
    };
    let pw_ok = upload_passwords.iter().any(|p| {
        if p.len() != pw.len() {
            return false;
        }
        subtle::ConstantTimeEq::ct_eq(p.as_bytes(), pw.as_bytes()).into()
    });
    if !pw_ok {
        return false;
    }
    if !ip_restricted {
        return true;
    }
    ip_in_list(upload_ip, client_ip)
}

pub fn challenge_admin_ip(allowed: &[String], client_ip: &str) -> bool {
    allowed.is_empty() || allowed.iter().any(|ip| ip == client_ip)
}
