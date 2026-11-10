//! 小米账号 → sid=micoapi serviceToken（MiNA 播控必需）。
//!
//! 支持：账号密码、扫码（longPolling + clientSign STS）。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use log::info;
use serde::Deserialize;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::{Result, SpeakerCastError};

const SID: &str = "micoapi";
const SERVICE_LOGIN: &str =
    "https://account.xiaomi.com/pass/serviceLogin?_json=true&sid=micoapi&_locale=zh_CN";
const LONG_POLL: &str = "https://account.xiaomi.com/longPolling/loginUrl";

/// micoapi 登录结果
#[derive(Debug, Clone)]
pub struct MicoApiLogin {
    pub user_id: String,
    pub service_token: String,
    pub pass_token: String,
    pub ssecurity: String,
    /// 到期毫秒；0 表示调用方填默认
    pub expire_at_ms: u64,
}

/// 扫码 begin 返回（给前端画二维码）
#[derive(Debug, Clone)]
pub struct QrLoginBegin {
    pub login_url: String,
    pub qr: String,
    pub lp: String,
    pub device_id: String,
}

/// Passport STS：base64(sha1("nonce={nonce}&{ssecurity}"))
pub fn client_sign(nonce: &str, ssecurity: &str) -> String {
    let dig = Sha1::digest(format!("nonce={nonce}&{ssecurity}").as_bytes());
    base64::engine::general_purpose::STANDARD.encode(dig)
}

/// 用小米账号密码换 micoapi serviceToken
pub async fn login_micoapi(user: &str, password: &str) -> Result<MicoApiLogin> {
    let user = user.trim();
    let password = password.trim();
    if user.is_empty() || password.is_empty() {
        return Err(SpeakerCastError::Mina("账号或密码为空".into()));
    }
    let http = passport_client()?;
    let device_id = random_device_id();

    let _ = http
        .get(SERVICE_LOGIN)
        .header("Cookie", format!("deviceId={device_id}; sdkVersion=3.9"))
        .send()
        .await;

    let step1 = http
        .get(SERVICE_LOGIN)
        .header("Cookie", format!("deviceId={device_id}; sdkVersion=3.9"))
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("serviceLogin: {e}")))?;
    let step1_json = parse_passport_json(&step1.text().await?)?;

    let mut form = HashMap::new();
    form.insert("_json", "true".to_string());
    form.insert(
        "qs",
        step1_json
            .get("qs")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    );
    form.insert("sid", SID.to_string());
    form.insert(
        "_sign",
        step1_json
            .get("_sign")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    );
    form.insert(
        "callback",
        step1_json
            .get("callback")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    );
    form.insert("user", user.to_string());
    form.insert("hash", md5_upper(password));

    let step2 = http
        .post("https://account.xiaomi.com/pass/serviceLoginAuth2")
        .header("Cookie", format!("deviceId={device_id}; sdkVersion=3.9"))
        .form(&form)
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("serviceLoginAuth2: {e}")))?;
    let auth = parse_passport_json(&step2.text().await?)?;
    let code = auth.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let desc = auth
            .get("desc")
            .or_else(|| auth.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("登录失败");
        return Err(SpeakerCastError::Mina(format!(
            "账号登录失败 code={code}: {desc}"
        )));
    }
    finish_sts(&http, &device_id, &auth).await
}

/// 发起扫码：拉 loginUrl / qr / lp（不阻塞等待）
pub async fn qr_login_begin() -> Result<QrLoginBegin> {
    let http = passport_client()?;
    let device_id = random_device_id();
    let headers = [
        ("User-Agent", ua()),
        ("Cookie", format!("deviceId={device_id}; sdkVersion=3.9")),
        ("Content-Type", "application/x-www-form-urlencoded".into()),
    ];

    let bootstrap = http
        .get(SERVICE_LOGIN)
        .header("User-Agent", &headers[0].1)
        .header("Cookie", &headers[1].1)
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("serviceLogin: {e}")))?;
    let data = parse_passport_json(&bootstrap.text().await?)?;
    let location = data.get("location").and_then(|v| v.as_str()).unwrap_or("");
    if data.get("code").and_then(|v| v.as_i64()) == Some(0) && !location.is_empty() {
        return Err(SpeakerCastError::Mina(
            "已有登录会话，请直接使用已有凭据或清 cookie 后重试".into(),
        ));
    }

    let mut q: HashMap<String, String> = url::Url::parse(location)
        .ok()
        .map(|u| {
            u.query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if q.is_empty() {
        // location 可能是相对或空：用 data 字段兜底
        for k in ["qs", "_sign", "callback", "sid"] {
            if let Some(v) = data.get(k).and_then(|x| x.as_str()) {
                q.insert(k.into(), v.into());
            }
        }
    }
    q.insert("theme".into(), String::new());
    q.insert("bizDeviceType".into(), String::new());
    q.insert("_hasLogo".into(), "false".into());
    q.insert("_qrsize".into(), "240".into());
    q.insert("_dc".into(), now_ms().to_string());

    let url = format!("{LONG_POLL}?{}", serde_urlencoded(&q));
    let login_ret = http
        .get(&url)
        .header("User-Agent", &headers[0].1)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("loginUrl: {e}")))?;
    let login_data = parse_passport_json(&login_ret.text().await?)?;
    if login_data.get("code").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
        return Err(SpeakerCastError::Mina(format!("拉二维码失败: {login_data}")));
    }
    let login_url = login_data
        .get("loginUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SpeakerCastError::Mina("无 loginUrl".into()))?
        .to_string();
    let qr = login_data
        .get("qr")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let lp = login_data
        .get("lp")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SpeakerCastError::Mina("无 lp".into()))?
        .to_string();
    Ok(QrLoginBegin {
        login_url,
        qr,
        lp,
        device_id,
    })
}

/// 长轮询扫码结果并完成 STS（阻塞至超时）
pub async fn qr_login_complete(begin: &QrLoginBegin) -> Result<MicoApiLogin> {
    let http = passport_client()?;
    let lp = http
        .get(&begin.lp)
        .header("User-Agent", ua())
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("扫码等待失败: {e}")))?;
    let lp_data = parse_passport_json(&lp.text().await?)?;
    if lp_data.get("passToken").is_none()
        && lp_data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) != 0
    {
        return Err(SpeakerCastError::Mina(format!("扫码结果异常: {lp_data}")));
    }
    finish_sts(&http, &begin.device_id, &lp_data).await
}

async fn finish_sts(
    _http: &reqwest::Client,
    device_id: &str,
    auth: &Value,
) -> Result<MicoApiLogin> {
    let user_id = json_id(auth, "userId")
        .ok_or_else(|| SpeakerCastError::Mina("登录响应无 userId".into()))?;
    let pass_token = auth
        .get("passToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ssecurity = auth
        .get("ssecurity")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SpeakerCastError::Mina("登录响应无 ssecurity".into()))?
        .to_string();
    let location = auth
        .get("location")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SpeakerCastError::Mina("登录响应无 location".into()))?
        .to_string();
    let nonce = json_id(auth, "nonce").unwrap_or_default();
    if nonce.is_empty() {
        return Err(SpeakerCastError::Mina("登录响应无 nonce".into()));
    }
    let sign = client_sign(&nonce, &ssecurity);
    let sep = if location.contains('?') { '&' } else { '?' };
    let sts_url = format!("{location}{sep}clientSign={sign}");
    // STS 必须关掉自动跳转：serviceToken 在 302 Set-Cookie 上，跟跳后响应头里没有
    let sts_http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .user_agent(ua())
        .build()
        .map_err(|e| SpeakerCastError::Mina(format!("sts client: {e}")))?;
    let mut service_token: Option<String> = None;
    let mut next = sts_url;
    for _ in 0..5 {
        let resp = sts_http
            .get(&next)
            .header("User-Agent", ua())
            .header("Cookie", format!("deviceId={device_id}; sdkVersion=3.9"))
            .send()
            .await
            .map_err(|e| SpeakerCastError::Mina(format!("STS: {e}")))?;
        service_token = cookie_from_response(&resp, "serviceToken").or(service_token);
        if service_token.is_some() {
            break;
        }
        let Some(loc) = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
        else {
            break;
        };
        next = if loc.starts_with("http") {
            loc
        } else {
            // 相对 Location
            let base = url::Url::parse(&next).ok();
            base.and_then(|b| b.join(&loc).ok())
                .map(|u| u.to_string())
                .unwrap_or(loc)
        };
    }
    let service_token = service_token
        .ok_or_else(|| SpeakerCastError::Mina("STS 未返回 serviceToken cookie".into()))?;

    let expire_at_ms = auth
        .get("expireTime")
        .and_then(|v| match v {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        })
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
                + 30 * 24 * 3600 * 1000
        });

    info!("micoapi login ok user_id={user_id}");
    Ok(MicoApiLogin {
        user_id,
        service_token,
        pass_token,
        ssecurity,
        expire_at_ms,
    })
}

/// 用 passToken 换新的 micoapi serviceToken
pub async fn refresh_micoapi(pass_token: &str, user_id: &str) -> Result<MicoApiLogin> {
    let pass_token = pass_token.trim();
    let user_id = user_id.trim();
    if pass_token.is_empty() || user_id.is_empty() {
        return Err(SpeakerCastError::Mina("缺少 passToken/userId，请重新扫码".into()));
    }
    let http = passport_client()?;
    let device_id = random_device_id();
    let r = http
        .get(SERVICE_LOGIN)
        .header("User-Agent", ua())
        .header(
            "Cookie",
            format!("deviceId={device_id}; sdkVersion=3.9;passToken={pass_token};userId={user_id}"),
        )
        .send()
        .await
        .map_err(|e| SpeakerCastError::Mina(format!("serviceLogin refresh: {e}")))?;
    let data = parse_passport_json(&r.text().await?)?;
    if data.get("code").and_then(|v| v.as_i64()) != Some(0) {
        return Err(SpeakerCastError::Mina(format!(
            "passToken 换票失败: {data}"
        )));
    }
    finish_sts(&http, &device_id, &data).await
}

/// 从响应头 + cookies() 解析指定 cookie
fn cookie_from_response(resp: &reqwest::Response, name: &str) -> Option<String> {
    if let Some(v) = cookie_from_headers(resp.headers(), name) {
        return Some(v);
    }
    resp.cookies()
        .find(|c| c.name() == name)
        .map(|c| c.value().to_string())
}

/// 从 Set-Cookie 头解析指定 cookie
fn cookie_from_headers(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    for val in headers.get_all(reqwest::header::SET_COOKIE) {
        let Ok(s) = val.to_str() else { continue };
        for part in s.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn passport_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(ua())
        .build()
        .map_err(|e| SpeakerCastError::Mina(format!("http client: {e}")))
}

fn parse_passport_json(raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    let json = trimmed.strip_prefix("&&&START&&&").unwrap_or(trimmed);
    serde_json::from_str(json).map_err(|e| {
        SpeakerCastError::Mina(format!(
            "passport json: {e}; body={}",
            &trimmed[..trimmed.len().min(160)]
        ))
    })
}

fn json_id(v: &Value, key: &str) -> Option<String> {
    v.get(key).map(|x| match x {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    })
    .filter(|s| !s.is_empty())
}

fn md5_upper(s: &str) -> String {
    format!("{:X}", md5::compute(s.as_bytes()))
}

fn random_device_id() -> String {
    (0..16)
        .map(|_| {
            let n = (rand_u8() % 36) as u8;
            if n < 10 {
                (b'0' + n) as char
            } else {
                (b'A' + (n - 10)) as char
            }
        })
        .collect()
}

fn rand_u8() -> u8 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    ((t ^ (t >> 7)) % 256) as u8
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn ua() -> String {
    format!(
        "Android-15-11.0.701-Xiaomi-23046RP50C-OS2.0.212.0.VMYCNXM-{}-CN-{}-{}-{}",
        hex_rand(40),
        hex_rand(32),
        hex_rand(32),
        hex_rand(40)
    )
}

fn hex_rand(n: usize) -> String {
    (0..n)
        .map(|_| {
            let x = rand_u8() % 16;
            char::from_digit(x as u32, 16)
                .unwrap_or('0')
                .to_ascii_uppercase()
        })
        .collect()
}

fn serde_urlencoded(map: &HashMap<String, String>) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in map {
        ser.append_pair(k, v);
    }
    ser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_sign_stable() {
        let s = client_sign("123", "abc");
        assert_eq!(s, client_sign("123", "abc"));
        assert!(!s.is_empty());
    }
}

#[derive(Debug, Deserialize)]
struct _Unused;
