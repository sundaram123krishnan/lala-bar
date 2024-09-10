use std::collections::HashMap;

use futures::future::pending;
use futures::StreamExt;
use iced::widget::{
    button, checkbox, column, container, image, row, scrollable, slider, svg, text, text_input,
    Space,
};
use iced::{executor, Font};
use iced::{Command, Element, Length, Theme};
use iced_layershell::actions::{
    LayershellCustomActionsWithIdAndInfo, LayershellCustomActionsWithInfo,
};
use iced_zbus_notification::{
    start_connection, ImageInfo, LaLaMako, NotifyMessage, NotifyUnit, VersionInfo, DEFAULT_ACTION,
    NOTIFICATION_SERVICE_PATH,
};
use launcher::{LaunchMessage, Launcher};
use zbus_mpirs::ServiceInfo;

use chrono::prelude::*;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::MultiApplication;
use iced_runtime::command::Action;
use iced_runtime::window::Action as WindowAction;

use futures::channel::mpsc::{channel, Receiver, Sender};

use tokio::sync::Mutex;

use std::sync::{Arc, LazyLock};

mod aximer;
mod launcher;
mod zbus_mpirs;

type LaLaShellIdAction = LayershellCustomActionsWithIdAndInfo<LaLaInfo>;
type LalaShellAction = LayershellCustomActionsWithInfo<LaLaInfo>;

const BEGINNING_UP_MARGIN: i32 = 10;

const UNIT_MARGIN: i32 = 135;

const EXTRAINF_MARGIN: i32 = BEGINNING_UP_MARGIN + 4 * UNIT_MARGIN;

const LAUNCHER_SVG: &[u8] = include_bytes!("../asserts/launcher.svg");

const RESET_SVG: &[u8] = include_bytes!("../asserts/reset.svg");

const ERROR_SVG: &[u8] = include_bytes!("../asserts/error.svg");

const GO_NEXT: &[u8] = include_bytes!("../asserts/go-next.svg");

static GO_NEXT_HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| svg::Handle::from_memory(GO_NEXT));

const GO_PREVIOUS: &[u8] = include_bytes!("../asserts/go-previous.svg");

static GO_PREVIOUS_HANDLE: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(GO_PREVIOUS));

const PLAY: &[u8] = include_bytes!("../asserts/play.svg");

static PLAY_HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| svg::Handle::from_memory(PLAY));

const PAUSE: &[u8] = include_bytes!("../asserts/pause.svg");

static PAUSE_HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| svg::Handle::from_memory(PAUSE));

const MAX_SHOWN_NOTIFICATIONS_COUNT: usize = 4;

pub fn main() -> Result<(), iced_layershell::Error> {
    env_logger::builder().format_timestamp(None).init();

    LalaMusicBar::run(Settings {
        layer_settings: LayerShellSettings {
            size: Some((0, 35)),
            exclusive_zone: 35,
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            ..Default::default()
        },
        ..Default::default()
    })
}

#[derive(Debug, Clone)]
enum LaLaInfo {
    Launcher,
    Notify(Box<NotifyUnitWidgetInfo>),
    HiddenInfo,
    RightPanel,
    ErrorHappened(iced::window::Id),
}

#[derive(Debug, Clone)]
struct NotifyUnitWidgetInfo {
    to_delete: bool,
    upper: i32,
    counter: usize,
    inline_reply: String,
    unit: NotifyUnit,
}

impl NotifyUnitWidgetInfo {
    fn notify_button<'a>(&self) -> Element<'a, Message> {
        let notify = &self.unit;
        let notify_theme = if notify.is_critical() {
            iced::theme::Button::Primary
        } else {
            iced::theme::Button::Secondary
        };
        match notify.image() {
            Some(ImageInfo::Svg(path)) => button(row![
                svg(svg::Handle::from_path(path))
                    .height(Length::Fill)
                    .width(Length::Fixed(70.)),
                Space::with_width(4.),
                column![
                    text(notify.summery.clone())
                        .shaping(text::Shaping::Advanced)
                        .size(20)
                        .font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        }),
                    text(notify.body.clone()).shaping(text::Shaping::Advanced)
                ]
            ])
            .style(notify_theme)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::RemoveNotify(self.unit.id))
            .into(),
            Some(ImageInfo::Data {
                width,
                height,
                pixels,
            }) => button(row![
                image(image::Handle::from_pixels(
                    width as u32,
                    height as u32,
                    pixels
                )),
                Space::with_width(4.),
                column![
                    text(notify.summery.clone())
                        .shaping(text::Shaping::Advanced)
                        .size(20)
                        .font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        }),
                    text(notify.body.clone()).shaping(text::Shaping::Advanced)
                ]
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(notify_theme)
            .on_press(Message::RemoveNotify(self.unit.id))
            .into(),
            Some(ImageInfo::Png(path)) | Some(ImageInfo::Jpg(path)) => button(row![
                image(image::Handle::from_path(path)).height(Length::Fill),
                Space::with_width(4.),
                column![
                    text(notify.summery.clone())
                        .shaping(text::Shaping::Advanced)
                        .size(20)
                        .font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        }),
                    text(notify.body.clone()).shaping(text::Shaping::Advanced)
                ]
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(iced::theme::Button::Secondary)
            .on_press(Message::RemoveNotify(self.unit.id))
            .into(),
            _ => button(column![
                text(notify.summery.clone()).shaping(text::Shaping::Advanced),
                text(notify.body.clone()).shaping(text::Shaping::Advanced)
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(notify_theme)
            .on_press(Message::RemoveNotify(self.unit.id))
            .into(),
        }
    }
}

#[allow(unused)]
#[derive(Debug)]
enum NotifyCommand {
    ActionInvoked { id: u32, action_key: String },
    InlineReply { id: u32, text: String },
    NotificationClosed { id: u32, reason: u32 },
}

struct LalaMusicBar {
    service_data: Option<ServiceInfo>,
    left: i64,
    right: i64,
    bar_index: SliderIndex,
    launcher: Option<launcher::Launcher>,
    launcherid: Option<iced::window::Id>,
    hidenid: Option<iced::window::Id>,
    right_panel: Option<iced::window::Id>,
    notifications: HashMap<u32, NotifyUnitWidgetInfo>,
    showned_notifications: HashMap<iced::window::Id, u32>,
    cached_notifications: HashMap<iced::window::Id, NotifyUnitWidgetInfo>,
    cached_hidden_notifications: Vec<NotifyUnitWidgetInfo>,
    sender: Sender<NotifyCommand>,
    receiver: Arc<Mutex<Receiver<NotifyCommand>>>,
    quite_mode: bool,

    datetime: DateTime<Local>,
}

impl LalaMusicBar {
    fn date_widget(&self) -> Element<Message> {
        let date = self.datetime.date_naive();
        let dateday = date.format("%m-%d").to_string();
        let week = date.format("%A").to_string();
        let time = self.datetime.time();
        let time_info = time.format("%H:%M").to_string();

        container(row![
            text(week),
            Space::with_width(2.),
            text(time_info),
            Space::with_width(2.),
            text(dateday)
        ])
        .center_x()
        .center_y()
        .height(Length::Fill)
        .into()
    }
    fn update_hidden_notification(&mut self) {
        let mut hiddened: Vec<NotifyUnitWidgetInfo> = self
            .notifications
            .values()
            .filter(|info| {
                (info.counter >= MAX_SHOWN_NOTIFICATIONS_COUNT || self.quite_mode)
                    && !info.to_delete
            })
            .cloned()
            .collect();

        hiddened.sort_by(|a, b| a.counter.partial_cmp(&b.counter).unwrap());

        self.cached_hidden_notifications = hiddened;
    }

    fn hidden_notification(&self) -> &Vec<NotifyUnitWidgetInfo> {
        &self.cached_hidden_notifications
    }
}

#[derive(Copy, Clone, Default)]
enum SliderIndex {
    #[default]
    Balance,
    Left,
    Right,
}

impl SliderIndex {
    fn next(&self) -> Self {
        match self {
            SliderIndex::Balance => SliderIndex::Left,
            SliderIndex::Left => SliderIndex::Right,
            SliderIndex::Right => SliderIndex::Balance,
        }
    }
    fn pre(&self) -> Self {
        match self {
            SliderIndex::Balance => SliderIndex::Right,
            SliderIndex::Left => SliderIndex::Balance,
            SliderIndex::Right => SliderIndex::Left,
        }
    }
}

impl LalaMusicBar {
    fn balance_percent(&self) -> u8 {
        if self.left == 0 && self.right == 0 {
            return 0;
        }
        (self.right * 100 / (self.left + self.right))
            .try_into()
            .unwrap()
    }

    fn update_balance(&mut self) {
        self.left = aximer::get_left().unwrap_or(0);
        self.right = aximer::get_right().unwrap_or(0);
    }

    fn set_balance(&mut self, balance: u8) {
        self.update_balance();
        let total = self.left + self.right;
        self.right = total * balance as i64 / 100;
        self.left = total - self.right;
        aximer::set_left(self.left);
        aximer::set_right(self.right);
    }
}

impl LalaMusicBar {
    // NOTE: not use signal to invoke remove, but use a common function
    fn remove_notify(&mut self, removed_id: u32) -> Command<Message> {
        let mut commands = vec![];
        let removed_counter = if let Some(removed_unit) = self.notifications.get_mut(&removed_id) {
            // NOTE: marked it as removable, but not now
            // Data should be removed by iced_layershell, not ourself
            removed_unit.to_delete = true;
            removed_unit.counter
        } else {
            // NOTE: already removed
            return Command::none();
        };

        for (_, notify_info) in self.notifications.iter_mut() {
            if notify_info.counter > removed_counter {
                notify_info.counter -= 1;
                notify_info.upper -= 135;
            }
        }

        let mut showned_values: Vec<(&iced::window::Id, &mut u32)> =
            self.showned_notifications.iter_mut().collect();

        showned_values.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap());

        let mut notifications: Vec<&u32> = self
            .notifications
            .iter()
            .filter(|(_, v)| !v.to_delete)
            .map(|(k, _)| k)
            .collect();

        notifications.sort_by(|a, b| b.partial_cmp(a).unwrap());

        let mut notification_iter = notifications.iter();

        let mut has_removed = false;

        for (id, nid) in showned_values.into_iter() {
            if let Some(onid) = notification_iter.next() {
                *nid = **onid;
            } else {
                if let Some(info) = self.notifications.get(nid) {
                    self.cached_notifications.insert(*id, info.clone());
                }
                *nid = removed_id;
                commands.push(Command::single(Action::Window(WindowAction::Close(*id))));
                has_removed = true;
            }
        }

        if !has_removed {
            self.notifications.retain(|_, v| !v.to_delete);
        }

        if self.quite_mode || removed_counter >= 4 {
            self.notifications.remove(&removed_id);
        }
        let notifications_count = self
            .notifications
            .iter()
            .filter(|(_, v)| !v.to_delete)
            .count();

        // NOTE: we should delete to be deleted notification
        if notifications_count <= MAX_SHOWN_NOTIFICATIONS_COUNT {
            if let Some(id) = self.hidenid {
                commands.push(Command::single(Action::Window(WindowAction::Close(id))));
            }
        }

        if notifications_count == 0 {
            commands.push(Command::perform(async {}, |_| Message::CheckOutput));
        }

        self.update_hidden_notification();

        Command::batch(commands)
    }
}

impl LalaMusicBar {
    fn balance_bar(&self) -> Element<Message> {
        let show_text = format!("balance {}%", self.balance_percent());
        row![
            button("<").on_press(Message::SliderIndexPre),
            Space::with_width(Length::Fixed(1.)),
            text(&show_text),
            Space::with_width(Length::Fixed(10.)),
            slider(0..=100, self.balance_percent(), Message::BalanceChanged),
            Space::with_width(Length::Fixed(10.)),
            button(
                svg(svg::Handle::from_memory(RESET_SVG))
                    .height(25.)
                    .width(25.)
            )
            .height(31.)
            .width(31.)
            .on_press(Message::BalanceChanged(50)),
            Space::with_width(Length::Fixed(1.)),
            button(">").on_press(Message::SliderIndexNext)
        ]
        .into()
    }
    fn left_bar(&self) -> Element<Message> {
        let show_text = format!("left {}%", self.left);
        row![
            button("<").on_press(Message::SliderIndexPre),
            Space::with_width(Length::Fixed(1.)),
            text(&show_text),
            Space::with_width(Length::Fixed(10.)),
            slider(0..=100, self.left as u8, Message::UpdateLeft),
            Space::with_width(Length::Fixed(10.)),
            button(">").on_press(Message::SliderIndexNext)
        ]
        .into()
    }
    fn right_bar(&self) -> Element<Message> {
        let show_text = format!("right {}%", self.right);
        row![
            button("<").on_press(Message::SliderIndexPre),
            Space::with_width(Length::Fixed(1.)),
            text(&show_text),
            Space::with_width(Length::Fixed(10.)),
            slider(0..=100, self.right as u8, Message::UpdateRight),
            Space::with_width(Length::Fixed(10.)),
            button(">").on_press(Message::SliderIndexNext)
        ]
        .into()
    }

    fn sound_slider(&self) -> Element<Message> {
        match self.bar_index {
            SliderIndex::Left => self.left_bar(),
            SliderIndex::Right => self.right_bar(),
            SliderIndex::Balance => self.balance_bar(),
        }
    }

    fn right_panel_view(&self) -> Element<Message> {
        let btns: Vec<Element<Message>> = self
            .hidden_notification()
            .iter()
            .map(|wdgetinfo| {
                container(wdgetinfo.notify_button())
                    .height(Length::Fixed(100.))
                    .into()
            })
            .collect();
        let mut view_elements: Vec<Element<Message>> = vec![];

        if let Some(data) = &self.service_data {
            let art_url_str = &data.metadata.mpris_arturl;
            if let Some(art_url) = url::Url::parse(art_url_str)
                .ok()
                .and_then(|url| url.to_file_path().ok())
            {
                // HACK: not render some thing like "/tmp/.org.chromium.Chromium.hYbnBf"
                if art_url_str.ends_with("png")
                    || art_url_str.ends_with("jpeg")
                    || art_url_str.ends_with("jpg")
                {
                    view_elements.push(
                        container(image(image::Handle::from_path(art_url)).width(Length::Fill))
                            .padding(10)
                            .width(Length::Fill)
                            .into(),
                    );
                }
                view_elements.push(Space::with_height(10.).into());
                view_elements.push(
                    container(
                        text(&data.metadata.xesam_title)
                            .size(20)
                            .font(Font {
                                weight: iced::font::Weight::Bold,
                                ..Default::default()
                            })
                            .shaping(text::Shaping::Advanced)
                            .style(iced::theme::Text::Color(iced::Color::WHITE)),
                    )
                    .width(Length::Fill)
                    .center_x()
                    .into(),
                );
                view_elements.push(Space::with_height(10.).into());
            }
        }
        view_elements.append(&mut vec![
            Space::with_height(10.).into(),
            scrollable(row!(
                Space::with_width(10.),
                column(btns).spacing(10.),
                Space::with_width(10.)
            ))
            .height(Length::Fill)
            .into(),
            container(checkbox("quite mode", self.quite_mode).on_toggle(Message::QuiteMode))
                .width(Length::Fill)
                .center_x()
                .into(),
            Space::with_height(10.).into(),
            container(button(text("clear all")).on_press(Message::ClearAllNotifications))
                .width(Length::Fill)
                .center_x()
                .into(),
            Space::with_height(10.).into(),
        ]);
        column(view_elements).into()
    }
}

#[derive(Debug, Clone)]
enum Message {
    RequestPre,
    RequestNext,
    RequestPause,
    RequestPlay,
    RequestDBusInfoUpdate,
    RequestUpdateTime,
    UpdateBalance,
    DBusInfoUpdate(Option<ServiceInfo>),
    BalanceChanged(u8),
    UpdateLeft(u8),
    UpdateRight(u8),
    SliderIndexNext,
    SliderIndexPre,
    ToggleLauncher,
    ToggleRightPanel,
    LauncherInfo(LaunchMessage),
    Notify(NotifyMessage),
    RemoveNotify(u32),
    InlineReply((u32, String)),
    InlineReplyMsgUpdate((iced::window::Id, String)),
    CheckOutput,
    ClearAllNotifications,
    QuiteMode(bool),
    CloseErrorNotification(iced::window::Id),
}

impl From<NotifyMessage> for Message {
    fn from(value: NotifyMessage) -> Self {
        Self::Notify(value)
    }
}

async fn get_metadata_initial() -> Option<ServiceInfo> {
    zbus_mpirs::init_mpirs().await.ok();
    get_metadata().await
}

async fn get_metadata() -> Option<ServiceInfo> {
    let infos = zbus_mpirs::MPIRS_CONNECTIONS.lock().await;

    let alive_infos: Vec<&ServiceInfo> = infos
        .iter()
        .filter(|info| !info.metadata.xesam_title.is_empty())
        .collect();

    if let Some(playingserver) = alive_infos
        .iter()
        .find(|info| info.playback_status == "Playing")
    {
        return Some((*playingserver).clone());
    }
    alive_infos.first().cloned().cloned()
}

impl LalaMusicBar {
    fn main_view(&self) -> Element<Message> {
        let toggle_launcher = button(
            svg(svg::Handle::from_memory(LAUNCHER_SVG))
                .width(25.)
                .height(25.),
        )
        .on_press(Message::ToggleLauncher);

        let sound_slider = self.sound_slider();
        let panel_text = if self.right_panel.is_some() { ">" } else { "<" };

        let Some(service_data) = &self.service_data else {
            let col = row![
                toggle_launcher,
                Space::with_width(Length::Fill),
                container(sound_slider).width(700.),
                Space::with_width(Length::Fixed(3.)),
                self.date_widget(),
                Space::with_width(Length::Fixed(3.)),
                button(text(panel_text)).on_press(Message::ToggleRightPanel)
            ]
            .spacing(10);
            return container(col)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x()
                .center_y()
                .into();
        };
        let title = &service_data.metadata.xesam_title;
        let art_url = 'out: {
            let art_url_str = &service_data.metadata.mpris_arturl;
            if !art_url_str.ends_with("png")
                && !art_url_str.ends_with("jpeg")
                && !art_url_str.ends_with("jpg")
            {
                break 'out None;
            }

            url::Url::parse(art_url_str)
                .ok()
                .and_then(|url| url.to_file_path().ok())
        };

        let title = container(
            text(title)
                .size(20)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                })
                .shaping(text::Shaping::Advanced)
                .style(iced::theme::Text::Color(iced::Color::WHITE)),
        )
        .width(Length::Fill)
        .center_x();
        let can_play = service_data.can_play;
        let can_pause = service_data.can_pause;
        let can_go_next = service_data.can_go_next;
        let can_go_pre = service_data.can_go_previous;
        let mut button_pre = button(svg(GO_PREVIOUS_HANDLE.clone()))
            .width(30.)
            .height(30.);
        if can_go_pre {
            button_pre = button_pre.on_press(Message::RequestPre);
        }
        let mut button_next = button(svg(GO_NEXT_HANDLE.clone())).width(30.).height(30.);
        if can_go_next {
            button_next = button_next.on_press(Message::RequestNext);
        }
        let button_play = if service_data.playback_status == "Playing" {
            let mut btn = button(svg(PAUSE_HANDLE.clone())).width(30.).height(30.);
            if can_pause {
                btn = btn.on_press(Message::RequestPause);
            }
            btn
        } else {
            let mut btn = button(svg(PLAY_HANDLE.clone())).width(30.).height(30.);
            if can_play {
                btn = btn.on_press(Message::RequestPlay);
            }
            btn
        };
        let buttons = container(row![button_pre, button_play, button_next].spacing(5))
            .width(Length::Fill)
            .center_x();

        let col = if let Some(art_url) = art_url {
            row![
                toggle_launcher,
                Space::with_width(Length::Fixed(5.)),
                image(image::Handle::from_path(art_url)),
                title,
                Space::with_width(Length::Fill),
                buttons,
                sound_slider,
                Space::with_width(Length::Fixed(3.)),
                self.date_widget(),
                Space::with_width(Length::Fixed(3.)),
                button(text(panel_text)).on_press(Message::ToggleRightPanel)
            ]
            .spacing(10)
        } else {
            row![
                toggle_launcher,
                title,
                Space::with_width(Length::Fill),
                buttons,
                sound_slider,
                Space::with_width(Length::Fixed(3.)),
                self.date_widget(),
                Space::with_width(Length::Fixed(3.)),
                button(text(panel_text)).on_press(Message::ToggleRightPanel)
            ]
            .spacing(10)
        };

        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
}

impl MultiApplication for LalaMusicBar {
    type Message = Message;
    type Flags = ();
    type Executor = executor::Default;
    type Theme = Theme;
    type WindowInfo = LaLaInfo;

    fn new(_flags: Self::Flags) -> (Self, Command<Message>) {
        let (sender, receiver) = channel::<NotifyCommand>(100);
        (
            Self {
                service_data: None,
                left: aximer::get_left().unwrap_or(0),
                right: aximer::get_right().unwrap_or(0),
                bar_index: SliderIndex::Balance,
                launcher: None,
                launcherid: None,
                right_panel: None,
                hidenid: None,
                notifications: HashMap::new(),
                showned_notifications: HashMap::new(),
                cached_notifications: HashMap::new(),
                cached_hidden_notifications: Vec::new(),
                sender,
                receiver: Arc::new(Mutex::new(receiver)),
                quite_mode: false,
                datetime: Local::now(),
            },
            Command::perform(get_metadata_initial(), Message::DBusInfoUpdate),
        )
    }

    fn namespace(&self) -> String {
        String::from("Mpirs_panel")
    }

    fn id_info(&self, id: iced::window::Id) -> Option<Self::WindowInfo> {
        if self.launcherid.is_some_and(|tid| tid == id) {
            Some(LaLaInfo::Launcher)
        } else if self.hidenid.is_some_and(|tid| tid == id) {
            Some(LaLaInfo::HiddenInfo)
        } else if self.right_panel.is_some_and(|tid| tid == id) {
            Some(LaLaInfo::RightPanel)
        } else {
            if let Some(info) = self.cached_notifications.get(&id) {
                return Some(LaLaInfo::Notify(Box::new(info.clone())));
            }
            let notify_id = self.showned_notifications.get(&id)?;
            Some(
                self.notifications
                    .get(notify_id)
                    .cloned()
                    .map(|notifyw| LaLaInfo::Notify(Box::new(notifyw)))
                    .unwrap_or(LaLaInfo::ErrorHappened(id)),
            )
        }
    }

    fn set_id_info(&mut self, id: iced::window::Id, info: Self::WindowInfo) {
        match info {
            LaLaInfo::Launcher => {
                self.launcherid = Some(id);
            }
            LaLaInfo::Notify(notify) => {
                self.showned_notifications.insert(id, notify.unit.id);
            }
            LaLaInfo::HiddenInfo => {
                self.hidenid = Some(id);
            }
            LaLaInfo::RightPanel => self.right_panel = Some(id),
            _ => unreachable!(),
        }
    }

    fn remove_id(&mut self, id: iced::window::Id) {
        if self.launcherid.is_some_and(|lid| lid == id) {
            self.launcherid.take();
            self.launcher.take();
        }
        if self.right_panel.is_some_and(|lid| lid == id) {
            self.right_panel.take();
        }
        if self.hidenid.is_some_and(|lid| lid == id) {
            self.hidenid.take();
        }
        'clear_nid: {
            if let Some(nid) = self.showned_notifications.remove(&id) {
                if let Some(NotifyUnitWidgetInfo {
                    to_delete: false, ..
                }) = self.notifications.get(&nid)
                {
                    break 'clear_nid;
                }
                // If the widget is marked to removed
                // Then delete it
                self.notifications.remove(&nid);
            }
        }
        self.cached_notifications.remove(&id);
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::DBusInfoUpdate(data) => self.service_data = data,
            Message::RequestDBusInfoUpdate => {
                return Command::perform(get_metadata(), Message::DBusInfoUpdate)
            }
            Message::RequestPlay => {
                if let Some(ref data) = self.service_data {
                    if !data.can_play {
                        return Command::none();
                    }
                    let data = data.clone();
                    return Command::perform(
                        async move {
                            data.play().await.ok();
                            get_metadata().await
                        },
                        Message::DBusInfoUpdate,
                    );
                }
            }
            Message::RequestPause => {
                if let Some(ref data) = self.service_data {
                    if !data.can_pause {
                        return Command::none();
                    }
                    let data = data.clone();
                    return Command::perform(
                        async move {
                            data.pause().await.ok();
                            get_metadata().await
                        },
                        Message::DBusInfoUpdate,
                    );
                }
            }
            Message::RequestPre => {
                if let Some(ref data) = self.service_data {
                    if !data.can_go_previous {
                        return Command::none();
                    }
                    let data = data.clone();
                    return Command::perform(
                        async move {
                            data.go_previous().await.ok();
                            get_metadata().await
                        },
                        Message::DBusInfoUpdate,
                    );
                }
            }
            Message::RequestNext => {
                if let Some(ref data) = self.service_data {
                    if !data.can_go_next {
                        return Command::none();
                    }
                    let data = data.clone();
                    return Command::perform(
                        async move {
                            data.go_next().await.ok();
                            get_metadata().await
                        },
                        Message::DBusInfoUpdate,
                    );
                }
            }
            Message::BalanceChanged(balance) => {
                let current_balance = self.balance_percent();
                if current_balance == 0 {
                    return Command::none();
                }
                self.set_balance(balance)
            }
            Message::UpdateBalance => {
                self.update_balance();
            }
            Message::UpdateLeft(percent) => {
                aximer::set_left(percent as i64);
                self.update_balance();
            }
            Message::UpdateRight(percent) => {
                aximer::set_right(percent as i64);
                self.update_balance();
            }
            Message::SliderIndexNext => self.bar_index = self.bar_index.next(),
            Message::SliderIndexPre => self.bar_index = self.bar_index.pre(),
            Message::ToggleLauncher => {
                if self.launcher.is_some() {
                    if let Some(id) = self.launcherid {
                        return Command::single(Action::Window(WindowAction::Close(id)));
                    }
                    return Command::none();
                }
                self.launcher = Some(Launcher::new());
                return Command::batch(vec![
                    Command::single(
                        LaLaShellIdAction::new(
                            iced::window::Id::MAIN,
                            LalaShellAction::NewLayerShell((
                                NewLayerShellSettings {
                                    size: Some((500, 700)),
                                    exclusive_zone: None,
                                    anchor: Anchor::Left | Anchor::Bottom,
                                    layer: Layer::Top,
                                    margin: None,
                                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                                    use_last_output: false,
                                },
                                LaLaInfo::Launcher,
                            )),
                        )
                        .into(),
                    ),
                    self.launcher.as_ref().unwrap().focus_input(),
                ]);
            }
            Message::ToggleRightPanel => {
                if self.right_panel.is_some() {
                    if let Some(id) = self.right_panel {
                        self.right_panel.take();
                        return Command::single(Action::Window(WindowAction::Close(id)));
                    }
                    return Command::none();
                }
                return Command::single(
                    LaLaShellIdAction::new(
                        iced::window::Id::MAIN,
                        LalaShellAction::NewLayerShell((
                            NewLayerShellSettings {
                                size: Some((300, 0)),
                                exclusive_zone: Some(300),
                                anchor: Anchor::Right | Anchor::Bottom | Anchor::Top,
                                layer: Layer::Top,
                                margin: None,
                                keyboard_interactivity: KeyboardInteractivity::None,
                                use_last_output: false,
                            },
                            LaLaInfo::RightPanel,
                        )),
                    )
                    .into(),
                );
            }
            Message::Notify(NotifyMessage::UnitAdd(notify)) => {
                if let Some(onotify) = self.notifications.get_mut(&notify.id) {
                    onotify.unit = *notify;
                    return Command::none();
                }
                let mut commands = vec![];
                for (_, notify) in self.notifications.iter_mut() {
                    notify.upper += 135;
                    notify.counter += 1;
                }

                // NOTE: support timeout
                if notify.timeout != -1 {
                    let timeout = notify.timeout as u64;
                    let id = notify.id;
                    commands.push(Command::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_secs(timeout)).await
                        },
                        move |_| Message::RemoveNotify(id),
                    ))
                }

                self.notifications.insert(
                    notify.id,
                    NotifyUnitWidgetInfo {
                        to_delete: false,
                        counter: 0,
                        upper: 10,
                        inline_reply: String::new(),
                        unit: *notify.clone(),
                    },
                );

                if !self.quite_mode {
                    let all_shown = self.showned_notifications.len() == 4;
                    let mut showned_notifications_now: Vec<(&u32, &NotifyUnitWidgetInfo)> = self
                        .notifications
                        .iter()
                        .filter(|(_, info)| info.counter < 4 && !info.to_delete)
                        .collect();

                    showned_notifications_now.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap());
                    let mut showned_values: Vec<(&iced::window::Id, &mut u32)> =
                        self.showned_notifications.iter_mut().collect();

                    showned_values.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap());

                    // NOTE: if all is shown, then do not add any commands
                    if all_shown {
                        let mut showned_values_iter = showned_values.iter_mut();

                        for (nid, _unit) in showned_notifications_now {
                            if let Some((_, onid)) = showned_values_iter.next() {
                                (**onid) = *nid;
                            }
                        }
                    } else {
                        // NOTE: if not all shown, then do as the way before
                        commands.push(Command::single(
                            LaLaShellIdAction::new(
                                iced::window::Id::MAIN,
                                LalaShellAction::NewLayerShell((
                                    NewLayerShellSettings {
                                        size: Some((300, 130)),
                                        exclusive_zone: None,
                                        anchor: Anchor::Right | Anchor::Top,
                                        layer: Layer::Top,
                                        margin: Some((10, 10, 10, 10)),
                                        keyboard_interactivity: KeyboardInteractivity::OnDemand,
                                        use_last_output: true,
                                    },
                                    LaLaInfo::Notify(Box::new(NotifyUnitWidgetInfo {
                                        to_delete: false,
                                        counter: 0,
                                        upper: 10,
                                        inline_reply: String::new(),
                                        unit: *notify.clone(),
                                    })),
                                )),
                            )
                            .into(),
                        ));

                        // NOTE: remove the new one
                        let to_adjust_notification = &showned_notifications_now[1..];
                        for ((_, unit), (id, _)) in
                            to_adjust_notification.iter().zip(showned_values.iter())
                        {
                            commands.push(Command::single(
                                LaLaShellIdAction::new(
                                    **id,
                                    LalaShellAction::MarginChange((unit.upper, 10, 10, 10)),
                                )
                                .into(),
                            ));
                        }
                    }
                }

                if self.notifications.len() > MAX_SHOWN_NOTIFICATIONS_COUNT
                    && self.hidenid.is_none()
                    && !self.quite_mode
                {
                    commands.push(Command::single(
                        LaLaShellIdAction::new(
                            iced::window::Id::MAIN,
                            LalaShellAction::NewLayerShell((
                                NewLayerShellSettings {
                                    size: Some((300, 25)),
                                    exclusive_zone: None,
                                    anchor: Anchor::Right | Anchor::Top,
                                    layer: Layer::Top,
                                    margin: Some((EXTRAINF_MARGIN, 10, 10, 10)),
                                    keyboard_interactivity: KeyboardInteractivity::None,
                                    use_last_output: true,
                                },
                                LaLaInfo::HiddenInfo,
                            )),
                        )
                        .into(),
                    ));
                }
                self.update_hidden_notification();
                return Command::batch(commands);
            }

            Message::QuiteMode(quite) => {
                self.quite_mode = quite;
                let mut commands = vec![];
                if quite {
                    for (id, _nid) in self.showned_notifications.iter() {
                        commands.push(Command::single(Action::Window(WindowAction::Close(*id))));
                    }
                    if let Some(extra_id) = self.hidenid {
                        commands.push(Command::single(Action::Window(WindowAction::Close(
                            extra_id,
                        ))));
                    }
                } else {
                    for (_, notify_info) in self
                        .notifications
                        .iter()
                        .filter(|(_, info)| info.counter < MAX_SHOWN_NOTIFICATIONS_COUNT)
                    {
                        commands.push(Command::single(
                            LaLaShellIdAction::new(
                                iced::window::Id::MAIN,
                                LalaShellAction::NewLayerShell((
                                    NewLayerShellSettings {
                                        size: Some((300, 130)),
                                        exclusive_zone: None,
                                        anchor: Anchor::Right | Anchor::Top,
                                        layer: Layer::Top,
                                        margin: Some((notify_info.upper, 10, 10, 10)),
                                        keyboard_interactivity: KeyboardInteractivity::OnDemand,
                                        use_last_output: true,
                                    },
                                    LaLaInfo::Notify(Box::new(notify_info.clone())),
                                )),
                            )
                            .into(),
                        ));
                    }
                    if self.notifications.len() > MAX_SHOWN_NOTIFICATIONS_COUNT
                        && self.hidenid.is_none()
                    {
                        commands.push(Command::single(
                            LaLaShellIdAction::new(
                                iced::window::Id::MAIN,
                                LalaShellAction::NewLayerShell((
                                    NewLayerShellSettings {
                                        size: Some((300, 25)),
                                        exclusive_zone: None,
                                        anchor: Anchor::Right | Anchor::Top,
                                        layer: Layer::Top,
                                        margin: Some((EXTRAINF_MARGIN, 10, 10, 10)),
                                        keyboard_interactivity: KeyboardInteractivity::None,
                                        use_last_output: true,
                                    },
                                    LaLaInfo::HiddenInfo,
                                )),
                            )
                            .into(),
                        ));
                    }
                }
                self.update_hidden_notification();

                return Command::batch(commands);
            }

            Message::Notify(NotifyMessage::UnitRemove(removed_id)) => {
                return self.remove_notify(removed_id)
            }

            Message::CheckOutput => {
                if self.notifications.is_empty() {
                    return Command::single(
                        LaLaShellIdAction::new(
                            iced::window::Id::MAIN,
                            LalaShellAction::ForgetLastOutput,
                        )
                        .into(),
                    );
                }
            }
            Message::LauncherInfo(message) => {
                if let Some(launcher) = self.launcher.as_mut() {
                    if let Some(id) = self.launcherid {
                        return launcher.update(message, id);
                    }
                }
            }
            Message::InlineReply((notify_id, text)) => {
                self.sender
                    .try_send(NotifyCommand::InlineReply {
                        id: notify_id,
                        text,
                    })
                    .ok();
                return self.remove_notify(notify_id);
            }
            Message::RemoveNotify(notify_id) => {
                self.sender
                    .try_send(NotifyCommand::ActionInvoked {
                        id: notify_id,
                        action_key: DEFAULT_ACTION.to_string(),
                    })
                    .ok();
                return self.remove_notify(notify_id);
            }
            Message::InlineReplyMsgUpdate((id, msg)) => {
                let Some(notify_id) = self.showned_notifications.get(&id) else {
                    return Command::none();
                };
                let notify = self.notifications.get_mut(notify_id).unwrap();
                notify.inline_reply = msg;
            }
            Message::ClearAllNotifications => {
                let mut commands = self
                    .showned_notifications
                    .keys()
                    .map(|id| Command::single(Action::Window(WindowAction::Close(*id))))
                    .collect::<Vec<_>>();
                self.notifications.clear();

                if let Some(id) = self.hidenid {
                    commands.push(Command::single(Action::Window(WindowAction::Close(id))));
                }

                commands.push(Command::perform(async {}, |_| Message::CheckOutput));
                return Command::batch(commands);
            }
            Message::CloseErrorNotification(id) => {
                return Command::single(Action::Window(WindowAction::Close(id)));
            }
            Message::RequestUpdateTime => {
                self.datetime = Local::now();
            }
        }
        Command::none()
    }

    fn view(&self, id: iced::window::Id) -> Element<Message> {
        if let Some(info) = self.id_info(id) {
            match info {
                LaLaInfo::Launcher => {
                    if let Some(launcher) = &self.launcher {
                        return launcher.view();
                    }
                }
                LaLaInfo::Notify(unitwidgetinfo) => {
                    let btnwidgets: Element<Message> = unitwidgetinfo.notify_button();

                    let notify = &unitwidgetinfo.unit;
                    if notify.inline_reply_support() {
                        return column![
                            btnwidgets,
                            Space::with_height(5.),
                            row![
                                text_input("reply something", &unitwidgetinfo.inline_reply)
                                    .on_input(move |msg| Message::InlineReplyMsgUpdate((id, msg)))
                                    .on_submit(Message::InlineReply((
                                        notify.id,
                                        unitwidgetinfo.inline_reply.clone()
                                    ))),
                                button("send").on_press(Message::InlineReply((
                                    notify.id,
                                    unitwidgetinfo.inline_reply.clone()
                                ))),
                            ]
                        ]
                        .into();
                    }
                    return btnwidgets;
                }
                LaLaInfo::HiddenInfo => {
                    return text(format!(
                        "hidden notifications {}",
                        self.hidden_notification().len()
                    ))
                    .into();
                }
                LaLaInfo::RightPanel => {
                    return self.right_panel_view();
                }
                LaLaInfo::ErrorHappened(id) => {
                    tracing::error!("Error happened, for window id: {id:?}");
                    return button(row![
                        svg(svg::Handle::from_memory(ERROR_SVG))
                            .height(Length::Fill)
                            .width(Length::Fixed(70.)),
                        Space::with_width(4.),
                        text("Error Happened, LaLa cannot find notification for this window, it is a bug, and should be fixed")
                    ]).on_press(Message::CloseErrorNotification(id)).into();
                }
            }
        }
        self.main_view()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        let rv = self.receiver.clone();
        iced::subscription::Subscription::batch([
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| Message::RequestDBusInfoUpdate),
            iced::time::every(std::time::Duration::from_secs(60))
                .map(|_| Message::RequestUpdateTime),
            iced::time::every(std::time::Duration::from_secs(5)).map(|_| Message::UpdateBalance),
            iced::event::listen()
                .map(|event| Message::LauncherInfo(LaunchMessage::IcedEvent(event))),
            iced::subscription::channel(std::any::TypeId::of::<()>(), 100, |sender| async move {
                let mut receiver = rv.lock().await;
                let Ok(connection) = start_connection(
                    sender,
                    vec![
                        "body".to_owned(),
                        "body-markup".to_owned(),
                        "actions".to_owned(),
                        "icon-static".to_owned(),
                        "x-canonical-private-synchronous".to_owned(),
                        "x-dunst-stack-tag".to_owned(),
                        "inline-reply".to_owned(),
                    ],
                    VersionInfo {
                        name: "LaLaMako".to_owned(),
                        vendor: "waycrate".to_owned(),
                        version: env!("CARGO_PKG_VERSION").to_owned(),
                        spec_version: env!("CARGO_PKG_VERSION_PATCH").to_owned(),
                    },
                )
                .await
                else {
                    pending::<()>().await;
                    unreachable!()
                };
                type LaLaMakoMusic = LaLaMako<Message>;
                let Ok(lalaref) = connection
                    .object_server()
                    .interface::<_, LaLaMakoMusic>(NOTIFICATION_SERVICE_PATH)
                    .await
                else {
                    pending::<()>().await;
                    unreachable!()
                };

                while let Some(cmd) = receiver.next().await {
                    match cmd {
                        NotifyCommand::ActionInvoked { id, action_key } => {
                            LaLaMakoMusic::action_invoked(
                                lalaref.signal_context(),
                                id,
                                &action_key,
                            )
                            .await
                            .ok();
                        }
                        NotifyCommand::InlineReply { id, text } => {
                            LaLaMakoMusic::notification_replied(
                                lalaref.signal_context(),
                                id,
                                &text,
                            )
                            .await
                            .ok();
                        }
                        NotifyCommand::NotificationClosed { id, reason } => {
                            LaLaMakoMusic::notification_closed(
                                lalaref.signal_context(),
                                id,
                                reason,
                            )
                            .await
                            .ok();
                        }
                    }
                }
                pending::<()>().await;
                unreachable!()
            }),
        ])
    }

    fn theme(&self) -> Self::Theme {
        Theme::TokyoNight
    }
}
