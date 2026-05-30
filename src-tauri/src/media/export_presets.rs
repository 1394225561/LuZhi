/// Fixed export preset selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportPreset {
    Bilibili,
    Douyin,
    Xiaohongshu,
}

/// Scaling policy for fixed MVP presets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportScalePolicy {
    FitWithBars,
    CenterCrop,
}

/// Immutable export settings for an MVP preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportPresetSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
    pub scale_policy: ExportScalePolicy,
}

impl ExportPreset {
    pub fn spec(self) -> ExportPresetSpec {
        match self {
            ExportPreset::Bilibili => ExportPresetSpec {
                id: "bilibili",
                display_name: "Bilibili / YouTube",
                width: 1920,
                height: 1080,
                fps: 30,
                video_bitrate_kbps: 8_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::FitWithBars,
            },
            ExportPreset::Douyin => ExportPresetSpec {
                id: "douyin",
                display_name: "抖音",
                width: 1080,
                height: 1920,
                fps: 30,
                video_bitrate_kbps: 8_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::CenterCrop,
            },
            ExportPreset::Xiaohongshu => ExportPresetSpec {
                id: "xiaohongshu",
                display_name: "小红书",
                width: 1080,
                height: 1080,
                fps: 30,
                video_bitrate_kbps: 6_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::CenterCrop,
            },
        }
    }
}

impl std::str::FromStr for ExportPreset {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "bilibili" => Ok(Self::Bilibili),
            "douyin" => Ok(Self::Douyin),
            "xiaohongshu" => Ok(Self::Xiaohongshu),
            other => Err(format!("未知导出预设：{other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_fixed_mvp_dimensions() {
        assert_eq!(ExportPreset::Bilibili.spec().width, 1920);
        assert_eq!(ExportPreset::Bilibili.spec().height, 1080);
        assert_eq!(ExportPreset::Douyin.spec().width, 1080);
        assert_eq!(ExportPreset::Douyin.spec().height, 1920);
        assert_eq!(ExportPreset::Xiaohongshu.spec().width, 1080);
        assert_eq!(ExportPreset::Xiaohongshu.spec().height, 1080);
    }

    #[test]
    fn preset_parse_rejects_templates_or_unknown_values() {
        assert_eq!("bilibili".parse::<ExportPreset>().unwrap(), ExportPreset::Bilibili);
        assert_eq!("douyin".parse::<ExportPreset>().unwrap(), ExportPreset::Douyin);
        assert_eq!(
            "xiaohongshu".parse::<ExportPreset>().unwrap(),
            ExportPreset::Xiaohongshu
        );
        assert!("template".parse::<ExportPreset>().is_err());
        assert!("youtube-shorts".parse::<ExportPreset>().is_err());
    }
}
