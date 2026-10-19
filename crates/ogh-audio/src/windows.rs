use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    eCapture, eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::core::{GUID, HRESULT, IUnknown, PCWSTR, PWSTR};

use crate::{AudioError, AudioRouter, Defaults, Direction, Role, Slot};

const SLOTS: &[Slot] = &[
    Slot::new(Direction::Render, Role::Console),
    Slot::new(Direction::Render, Role::Multimedia),
    Slot::new(Direction::Render, Role::Communications),
    Slot::new(Direction::Capture, Role::Console),
    Slot::new(Direction::Capture, Role::Multimedia),
    Slot::new(Direction::Capture, Role::Communications),
];

const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

mod policy_config {
    #![allow(non_snake_case)]

    use windows::Win32::Media::Audio::ERole;
    use windows::core::{BOOL, HRESULT, PCWSTR};
    use windows_core::{IUnknown, IUnknown_Vtbl};

    /// The undocumented policy interface behind the Sound control panel. Method
    /// order follows EarTrumpet's `IPolicyConfig.cs`: eight methods this code
    /// never calls, `GetPropertyValue`, `SetPropertyValue`, then
    /// `SetDefaultEndpoint` as the 11th after `IUnknown`'s three. Only
    /// `SetDefaultEndpoint` is called; the others just keep the vtable aligned.
    #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub(super) unsafe trait IPolicyConfig: IUnknown {
        unsafe fn Unused1(&self) -> HRESULT;
        unsafe fn Unused2(&self) -> HRESULT;
        unsafe fn Unused3(&self) -> HRESULT;
        unsafe fn Unused4(&self) -> HRESULT;
        unsafe fn Unused5(&self) -> HRESULT;
        unsafe fn Unused6(&self) -> HRESULT;
        unsafe fn Unused7(&self) -> HRESULT;
        unsafe fn Unused8(&self) -> HRESULT;
        unsafe fn GetPropertyValue(&self) -> HRESULT;
        unsafe fn SetPropertyValue(&self) -> HRESULT;
        pub unsafe fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
        unsafe fn SetEndpointVisibility(&self, device_id: PCWSTR, visible: BOOL) -> HRESULT;
    }
}

use policy_config::IPolicyConfig;

/// Holds no COM objects: each call makes its own, so it can move between
/// threads. The calling thread must have run `init_thread`.
pub struct WindowsRouter;

impl AudioRouter for WindowsRouter {
    fn slots(&self) -> &'static [Slot] {
        SLOTS
    }

    fn current(&self) -> Result<Defaults, AudioError> {
        let enumerator = enumerator()?;
        let mut defaults = Defaults::new();
        for &slot in SLOTS {
            // SAFETY: `enumerator` is a live COM object; the arguments are plain values.
            let result = unsafe {
                enumerator.GetDefaultAudioEndpoint(data_flow(slot.direction), role(slot.role))
            };
            match result {
                Ok(device) => {
                    defaults.insert(slot, device_id(&device)?);
                }
                Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => {}
                Err(e) => return Err(failed("cannot read the default audio device", &e)),
            }
        }
        Ok(defaults)
    }

    fn find(
        &self,
        direction: Direction,
        name_contains: &str,
    ) -> Result<Option<String>, AudioError> {
        let enumerator = enumerator()?;
        let needle = name_contains.to_lowercase();
        // SAFETY: `enumerator` is a live COM object; the arguments are plain values.
        let devices =
            unsafe { enumerator.EnumAudioEndpoints(data_flow(direction), DEVICE_STATE_ACTIVE) }
                .map_err(|e| failed("cannot list audio devices", &e))?;
        // SAFETY: `devices` is a live COM object.
        let count =
            unsafe { devices.GetCount() }.map_err(|e| failed("cannot count audio devices", &e))?;
        for index in 0..count {
            // SAFETY: `index` is below the count just read.
            let device = unsafe { devices.Item(index) }
                .map_err(|e| failed("cannot read an audio device", &e))?;
            if friendly_name(&device)?.to_lowercase().contains(&needle) {
                return device_id(&device).map(Some);
            }
        }
        Ok(None)
    }

    fn set(&self, slot: Slot, endpoint: &str) -> Result<(), AudioError> {
        // SAFETY: the CLSID names the policy config client and the requested
        // interface is declared with the matching IID above.
        let policy: IPolicyConfig =
            unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None::<&IUnknown>, CLSCTX_ALL) }
                .map_err(|e| failed("cannot reach the audio policy", &e))?;
        let wide: Vec<u16> = endpoint.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        unsafe { policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role(slot.role)) }
            .ok()
            .map_err(|e| failed(&format!("cannot set the default for {slot}"), &e))
    }
}

pub(crate) fn init_thread() {
    // SAFETY: no pointers are involved. The result is ignored: S_FALSE means
    // the thread was already initialised and RPC_E_CHANGED_MODE that another
    // apartment model is in place, and both leave COM usable.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // SAFETY: the CLSID and interface type belong together.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL) }
        .map_err(|e| failed("cannot create the audio device enumerator", &e))
}

fn friendly_name(device: &IMMDevice) -> Result<String, AudioError> {
    let context = "cannot read an audio device name";
    // SAFETY: `device` is a live COM object.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.map_err(|e| failed(context, &e))?;
    // SAFETY: the key is a valid constant; the returned PROPVARIANT clears itself on drop.
    let value =
        unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.map_err(|e| failed(context, &e))?;
    // SAFETY: `value` is a valid PROPVARIANT.
    let text = unsafe { PropVariantToStringAlloc(&value) }.map_err(|e| failed(context, &e))?;
    take_string(text)
}

fn device_id(device: &IMMDevice) -> Result<String, AudioError> {
    // SAFETY: `device` is a live COM object.
    let id = unsafe { device.GetId() }.map_err(|e| failed("cannot read an audio device id", &e))?;
    take_string(id)
}

/// Copies a COM-allocated wide string and frees it.
fn take_string(text: PWSTR) -> Result<String, AudioError> {
    // SAFETY: `text` came from a COM call that returns a NUL-terminated string
    // allocated for the caller to free with `CoTaskMemFree`, freed once here.
    let result = unsafe { text.to_string() };
    unsafe { CoTaskMemFree(Some(text.as_ptr().cast_const().cast())) };
    result.map_err(|e| AudioError::Failed(format!("audio device text is not valid: {e}")))
}

fn data_flow(direction: Direction) -> EDataFlow {
    match direction {
        Direction::Render => eRender,
        Direction::Capture => eCapture,
    }
}

fn role(role: Role) -> ERole {
    match role {
        Role::Console => eConsole,
        Role::Multimedia => eMultimedia,
        Role::Communications => eCommunications,
    }
}

fn failed(context: &str, error: &windows::core::Error) -> AudioError {
    AudioError::Failed(format!("{context}: {error}"))
}
