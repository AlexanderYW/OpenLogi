//! Device display controls: the `0x8040` BrightnessControl slider.
//!
//! Brightness is read from the device when it is selected and shown as the
//! value the device echoes; a committed level is also persisted, so the agent
//! can restore it on a device that forgets it. The MX Creative Keypad reports
//! it for its LCD keys; any device reporting the feature gets the same panel,
//! gated on the measured `brightness` capability.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window,
};
use gpui_component::{Icon, IconName, h_flex, slider::Slider, v_flex};

use openlogi_core::hid::WritableBrightness;

use crate::state::{AppState, BrightnessLoad, DeviceKey, DeviceRecord, StateEvent};
use crate::ui::commit_slider::{CommitSlider, SliderRange};
use crate::ui::components::PanelCard;
use crate::ui::status::{retry_line, status_line};
use crate::ui::theme::{self, Palette, Typography as _};

/// The slider together with what it was built for. The range is the levels
/// OpenLogi may write to the device — its advertised range without `0` — so
/// the slider is built from the first read that reports one and rebuilt only
/// when the device or its range changes.
struct BrightnessSlider {
    device_key: DeviceKey,
    range: WritableBrightness,
    slider: CommitSlider<u16>,
}

/// The device-brightness card on the Device tab.
pub struct BrightnessPanel {
    slider: Option<BrightnessSlider>,
    _state_obs: Subscription,
}

impl BrightnessPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state_obs = AppState::repaint_on(cx, |event| {
            matches!(event, StateEvent::BrightnessChanged(_))
        });
        Self {
            slider: None,
            _state_obs: state_obs,
        }
    }

    /// Keep the slider pointed at the device on screen and seated on its
    /// echoed level: built from the first read that reports a range, re-seated
    /// after every echo, never moved under a drag.
    fn sync_slider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = AppState::try_read(cx);
        let (Some(record), load) = (
            state.and_then(AppState::current_record).cloned(),
            state.map(AppState::current_brightness_load),
        ) else {
            self.slider = None;
            return;
        };
        let device_key = record.device_key();
        let BrightnessLoad::Ready(reading) = load.unwrap_or_default() else {
            if self
                .slider
                .as_ref()
                .is_some_and(|slider| slider.device_key != device_key)
            {
                self.slider = None;
            }
            return;
        };
        let range = reading.writable();
        match &self.slider {
            Some(slider) if slider.device_key == device_key && slider.range == range => {
                slider.slider.sync(reading.level, window, cx);
            }
            _ => {
                let slider = CommitSlider::new(
                    SliderRange::new(range.min(), range.max()),
                    reading.level,
                    cx,
                    |_, level, cx| {
                        AppState::apply(cx, |state| state.commit_brightness(level));
                    },
                );
                self.slider = Some(BrightnessSlider {
                    device_key,
                    range,
                    slider,
                });
            }
        }
    }
}

impl Render for BrightnessPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_slider(window, cx);
        let pal = theme::palette(cx);
        let body = brightness_body(self, pal, cx);
        PanelCard::new(tr!("device.brightness"), Icon::new(IconName::Sun), body)
    }
}

/// The card body, one per load state, rendered the way the DPI and SmartShift
/// panels render theirs: the slider once the device's range has landed, a
/// reading/offline line while it has not, a retry line when the read failed,
/// and a quiet line if the device turned out not to have the control.
fn brightness_body(
    panel: &BrightnessPanel,
    pal: Palette,
    cx: &mut Context<BrightnessPanel>,
) -> AnyElement {
    let (load, reachable, key) = AppState::try_read(cx).map_or_else(
        || (BrightnessLoad::Unknown, false, None),
        |state| {
            let record = state.current_record();
            (
                state.current_brightness_load(),
                record.is_some_and(|record| record.route.is_some()),
                record.map(DeviceRecord::device_key),
            )
        },
    );
    match load {
        BrightnessLoad::Ready(reading) => panel.slider.as_ref().map_or_else(
            || status_line(tr!("device.brightness_reading"), pal).into_any_element(),
            |slider| {
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_end()
                            .text_caption()
                            .text_color(pal.text_primary)
                            .child(slider.slider.shown(reading.level).to_string()),
                    )
                    .child(Slider::new(slider.slider.slider()).horizontal())
                    .into_any_element()
            },
        ),
        BrightnessLoad::Loading | BrightnessLoad::Unknown => status_line(
            if reachable {
                tr!("device.brightness_reading")
            } else {
                tr!("device.brightness_offline")
            },
            pal,
        )
        .into_any_element(),
        BrightnessLoad::Failed(_) => key.map_or_else(
            || status_line(tr!("device.brightness_read_failed"), pal).into_any_element(),
            |key| {
                retry_line(
                    "brightness-retry",
                    tr!("device.brightness_read_failed"),
                    pal,
                    move |cx| {
                        AppState::apply(cx, |state| state.retry_brightness_read(&key));
                    },
                )
                .into_any_element()
            },
        ),
        BrightnessLoad::Unsupported(_) => {
            status_line(tr!("device.brightness_unsupported"), pal).into_any_element()
        }
    }
}
