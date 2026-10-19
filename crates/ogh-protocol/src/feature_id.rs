#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId(pub u16);

impl std::fmt::Debug for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FeatureId({:#06x})", self.0)
    }
}

macro_rules! feature_ids {
    ($($const_name:ident = $id:literal => $name:literal,)*) => {
        impl FeatureId {
            $(pub const $const_name: FeatureId = FeatureId($id);)*

            /// `None` for ids with no known meaning on the A50 X, such as
            /// 0x0102, 0x0303 and 0x0805 to 0x0807.
            pub fn name(self) -> Option<&'static str> {
                match self.0 {
                    $($id => Some($name),)*
                    _ => None,
                }
            }
        }
    };
}

feature_ids! {
    ROOT = 0x0000 => "Root",
    FEATURE_SET = 0x0001 => "FeatureSet",
    CENT_PP_BRIDGE = 0x0003 => "CentPPBridge",
    PROFILE = 0x0004 => "Profile",
    MULTIHOST_CONTROL = 0x0005 => "MultihostControl",
    KEEP_ALIVE = 0x0007 => "KeepAlive",
    DEVICE_INFO = 0x0100 => "DeviceInfo",
    DEVICE_NAME = 0x0101 => "DeviceName",
    MEMFAULT = 0x0103 => "Memfault",
    BATTERY_SOC = 0x0104 => "BatterySoc",
    AUTO_SLEEP = 0x0108 => "AutoSleep",
    GENERIC_DFU = 0x010A => "GenericDfu",
    LED_BRIGHTNESS = 0x0110 => "LedBrightness",
    EU_POWER_MODE_CONTROL = 0x0115 => "EuPowerModeControl",
    DEVICE_BOOL_STATE = 0x0116 => "DeviceBoolState",
    VOLUME_SET = 0x0200 => "VolumeSet",
    EQ_SET = 0x0201 => "EqSet",
    ADVANCED_PARA_EQ = 0x020D => "AdvancedParaEq",
    MIC_TEST = 0x020E => "MicTest",
    BT_HOST_INFO = 0x0305 => "BtHostInfo",
    LIGHTSPEED_PAIRING = 0x0309 => "LightSpeedPairing",
    RGB_EFFECTS = 0x0600 => "RgbEffects",
    HEADSET_MIC_MUTE = 0x0601 => "HeadsetMicMute",
    HEADSET_MIC_SNR = 0x0602 => "HeadsetMicSnr",
    HEADSET_SIDETONE = 0x0604 => "HeadsetSidetone",
    HEADSET_ANC = 0x0605 => "HeadsetAnc",
    HEADSET_HOST_SWITCH = 0x0607 => "HeadsetHostSwitch",
    HEADSET_MIX = 0x0609 => "HeadsetMix",
    HEADSET_TONES = 0x060B => "HeadsetTones",
    NOISE_EXPOSURE = 0x060D => "NoiseExposure",
    AI_NOISE_REDUCTION = 0x060E => "AiNoiseReduction",
    HEADSET_MIC_GAIN = 0x0611 => "HeadsetMicGain",
    HEADSET_USAGE_TRACKING = 0x0617 => "HeadsetUsageTracking",
    HEADSET_BATTERY_SAVER = 0x0618 => "HeadsetBatterySaver",
    DO_NOT_DISTURB = 0x0631 => "DoNotDisturb",
    ONBOARD_PROFILES = 0x0634 => "OnboardProfiles",
    RGB_STREAMING = 0x0635 => "RgbStreaming",
    ONBOARD_EQ = 0x0636 => "OnboardEq",
    FOOTSTEP_DETECTION = 0x0639 => "FootstepDetection",
    USB_PORT_CONFIG = 0x063A => "UsbPortConfig",
    MIC_SELECT = 0x063B => "MicSelect",
    AUX_CONFIG = 0x063C => "AuxConfig",
    MIXER_AUDIO = 0x0800 => "MixerAudio",
    MIXER_MIC = 0x0801 => "MixerMic",
    ADVANCED_MIXER_AUDIO = 0x0809 => "AdvancedMixerAudio",
    AUDIO_INPUT = 0x080A => "AudioInput",
}
