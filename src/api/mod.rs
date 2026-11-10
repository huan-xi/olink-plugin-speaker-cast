//! HTTP：小米多账号 + 音响接入 + 开/关 AirPlay（不接 HAP）。

use std::sync::{Arc, OnceLock};

use axum::routing::{delete, get, post};
use axum::{Json, Router};
use crate::api_resp::ApiResp;

use crate::access::SpeakerAccess;
use crate::bridge::MP3_BITRATE_KBPS;
use crate::service::SpeakerCastService;
use crate::speaker::QrSessionStatus;

use self::request::{ConnectSpeakerReq, DidReq, MinaQrPollReq, MinaTokenReq, TestPlayReq};
use self::response::{
    AccountResp, CastStatusResp, MinaDeviceResp, MinaQrBeginResp, MinaQrPollResp, SpeakerAccessResp,
};

pub mod request;
pub mod response;

static STATE: OnceLock<Arc<SpeakerCastService>> = OnceLock::new();

/// 注入服务
pub fn set_service(svc: Arc<SpeakerCastService>) {
    let _ = STATE.set(svc);
}

fn svc() -> Arc<SpeakerCastService> {
    STATE.get().expect("speaker-cast service").clone()
}

/// 业务路由（挂在插件 UDS；Host proxy 前缀外露）
pub fn router() -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/accounts", get(list_accounts))
        .route("/accounts/{user_id}", delete(delete_account))
        .route("/accounts/{user_id}/refresh", post(refresh_account))
        .route("/accounts/{user_id}/devices", get(list_account_devices))
        .route("/speakers", get(list_speakers))
        .route("/access", post(connect_speaker))
        .route("/access/{did}", delete(disconnect_speaker))
        .route("/airplay/enable", post(enable_airplay))
        .route("/airplay/disable", post(disable_airplay))
        .route("/mina/token", post(set_mina_token))
        .route("/mina/qr/begin", post(mina_qr_begin))
        .route("/mina/qr/poll", post(mina_qr_poll))
        .route("/test/play", post(test_play))
        .route("/test/stop", post(test_stop))
}

async fn status() -> ApiResp<CastStatusResp> {
    let s = svc();
    let accounts = s.list_accounts().await;
    ApiResp::with_data(CastStatusResp {
        mina_ready: s.mina_ready().await,
        accounts_count: accounts.len(),
        bridge_port: s.bridge.port(),
        receivers: s.airplay.list_status().await,
    })
}

async fn list_accounts() -> ApiResp<Vec<AccountResp>> {
    ApiResp::with_data(svc().list_accounts().await)
}

async fn delete_account(
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> ApiResp<()> {
    match svc().delete_account(user_id.trim()).await {
        Ok(()) => ApiResp::with_msg("ok"),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn refresh_account(
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> ApiResp<AccountResp> {
    match svc().refresh_account(user_id.trim()).await {
        Ok(a) => ApiResp::with_data(a),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn list_account_devices(
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> ApiResp<Vec<MinaDeviceResp>> {
    match svc().list_mina_devices_for(user_id.trim()).await {
        Ok(list) => ApiResp::with_data(list.into_iter().map(Into::into).collect()),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn enrich_access(row: SpeakerAccess) -> SpeakerAccessResp {
    let s = svc();
    let receivers = s.airplay.list_status().await;
    let recv = receivers.iter().find(|r| r.did == row.did);
    let (sample_rate, channels, streaming) = match s.pcm.get(&row.did).await {
        Some(p) => (p.rate(), p.ch(), p.streaming(3_000)),
        None => (0, 0, false),
    };
    SpeakerAccessResp::from_access(
        row,
        s.bridge.port(),
        recv.map(|r| r.running).unwrap_or(false),
        recv.map(|r| r.raop_port).unwrap_or(0),
        MP3_BITRATE_KBPS,
        sample_rate,
        channels,
        streaming,
    )
}

async fn list_speakers() -> ApiResp<Vec<SpeakerAccessResp>> {
    let list = svc().access.list().await;
    let mut out = Vec::with_capacity(list.len());
    for row in list {
        out.push(enrich_access(row).await);
    }
    ApiResp::with_data(out)
}

async fn connect_speaker(Json(req): Json<ConnectSpeakerReq>) -> ApiResp<SpeakerAccessResp> {
    match svc()
        .connect(
            &req.did,
            req.name.as_deref().unwrap_or(""),
            req.mina_device_id.as_deref(),
            req.airplay_name,
            &req.account_id,
        )
        .await
    {
        Ok(s) => ApiResp::with_data(enrich_access(s).await),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn disconnect_speaker(
    axum::extract::Path(did): axum::extract::Path<String>,
) -> ApiResp<()> {
    match svc().disconnect(&did).await {
        Ok(()) => ApiResp::with_msg("ok"),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn enable_airplay(Json(req): Json<DidReq>) -> ApiResp<SpeakerAccessResp> {
    match svc().enable_airplay(req.did.trim()).await {
        Ok(s) => ApiResp::with_data(enrich_access(s).await),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn disable_airplay(Json(req): Json<DidReq>) -> ApiResp<SpeakerAccessResp> {
    match svc().disable_airplay(req.did.trim()).await {
        Ok(s) => ApiResp::with_data(enrich_access(s).await),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn set_mina_token(Json(req): Json<MinaTokenReq>) -> ApiResp<AccountResp> {
    match svc()
        .upsert_account_tokens(
            req.user_id,
            req.service_token,
            req.pass_token.unwrap_or_default(),
        )
        .await
    {
        Ok(a) => ApiResp::with_data(a),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn mina_qr_begin() -> ApiResp<MinaQrBeginResp> {
    match svc().mina_qr_begin().await {
        Ok((session_id, login_url, qr)) => ApiResp::with_data(MinaQrBeginResp {
            session_id,
            login_url,
            qr,
        }),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn mina_qr_poll(Json(req): Json<MinaQrPollReq>) -> ApiResp<MinaQrPollResp> {
    match svc().mina_qr_poll(req.session_id.trim()).await {
        Ok(QrSessionStatus::Pending { .. }) => ApiResp::with_data(MinaQrPollResp {
            status: "pending".into(),
            message: None,
            user_id: None,
        }),
        Ok(QrSessionStatus::Ok(login)) => ApiResp::with_data(MinaQrPollResp {
            status: "ok".into(),
            message: None,
            user_id: Some(login.user_id),
        }),
        Ok(QrSessionStatus::Err(msg)) => ApiResp::with_data(MinaQrPollResp {
            status: "error".into(),
            message: Some(msg),
            user_id: None,
        }),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn test_play(Json(req): Json<TestPlayReq>) -> ApiResp<()> {
    match svc().test_play(req.did.trim(), req.url.trim()).await {
        Ok(()) => ApiResp::with_msg("ok"),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}

async fn test_stop(Json(req): Json<DidReq>) -> ApiResp<()> {
    match svc().test_stop(req.did.trim()).await {
        Ok(()) => ApiResp::with_msg("ok"),
        Err(e) => ApiResp::failure(&e.to_string()),
    }
}
