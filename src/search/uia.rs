//! UI Automation setup shared by the Explorer and browser tab code.
//!
//! Lives on the worker thread (an STA); its COM objects never leave it.

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationCondition,
    IUIAutomationElement, IUIAutomationSelectionItemPattern, UIA_ControlTypePropertyId, UIA_NamePropertyId,
    UIA_SelectionItemIsSelectedPropertyId, UIA_SelectionItemPatternId, UIA_TabItemControlTypeId,
};
use windows::core::Interface;

/// Cap so one hung Explorer or browser can't stall UI Automation calls for long.
pub const UIA_TIMEOUT_MS: u32 = 1500;

pub struct Uia {
    pub automation: IUIAutomation,
    /// Control type is TabItem.
    pub tab_condition: IUIAutomationCondition,
    /// Fetches a tab item's name and selected state with the element.
    pub tab_cache: IUIAutomationCacheRequest,
}

impl Uia {
    /// Needs COM initialised on this thread.
    pub fn new() -> windows::core::Result<Uia> {
        unsafe {
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            if let Ok(a2) = automation.cast::<IUIAutomation2>() {
                let _ = a2.SetConnectionTimeout(UIA_TIMEOUT_MS);
                let _ = a2.SetTransactionTimeout(UIA_TIMEOUT_MS);
            }
            let tab_condition = automation
                .CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TabItemControlTypeId.0))?;
            let tab_cache = automation.CreateCacheRequest()?;
            tab_cache.AddProperty(UIA_NamePropertyId)?;
            tab_cache.AddProperty(UIA_SelectionItemIsSelectedPropertyId)?;
            Ok(Uia {
                automation,
                tab_condition,
                tab_cache,
            })
        }
    }
}

/// Select a tab item with its SelectionItem pattern.
pub fn select_element(el: &IUIAutomationElement) -> windows::core::Result<()> {
    unsafe {
        let p = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)?;
        p.Select()
    }
}

/// Whether a tab item reports itself as selected (false when it can't say).
pub fn is_selected(el: &IUIAutomationElement) -> bool {
    unsafe {
        el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
            .and_then(|p| p.CurrentIsSelected())
            .is_ok_and(|b| b.as_bool())
    }
}

/// An element's current name, or "" when it can't be read.
pub fn name(el: &IUIAutomationElement) -> String {
    unsafe { el.CurrentName() }.map(|b| b.to_string()).unwrap_or_default()
}
