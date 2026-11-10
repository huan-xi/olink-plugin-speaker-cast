//! AirPlay 音量（dB）→ 音箱百分数。

/// AirPlay SET_PARAMETER 音量：`0.0` = 最大，`-144.0` = 静音。
/// 常用可调区间约 `-30..=0` dB，线性映射到 `0..=100`。
pub fn airplay_db_to_percent(db: f32) -> u8 {
    if !db.is_finite() || db <= -144.0 {
        return 0;
    }
    let db = db.clamp(-30.0, 0.0);
    ((db + 30.0) / 30.0 * 100.0).round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_is_100() {
        assert_eq!(airplay_db_to_percent(0.0), 100);
    }

    #[test]
    fn mute_is_0() {
        assert_eq!(airplay_db_to_percent(-144.0), 0);
        assert_eq!(airplay_db_to_percent(-30.0), 0);
    }

    #[test]
    fn mid_is_50() {
        assert_eq!(airplay_db_to_percent(-15.0), 50);
    }

    #[test]
    fn below_range_clamps_to_0() {
        assert_eq!(airplay_db_to_percent(-40.0), 0);
    }

    #[test]
    fn nan_is_0() {
        assert_eq!(airplay_db_to_percent(f32::NAN), 0);
    }
}
