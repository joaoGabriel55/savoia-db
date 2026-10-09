//! New / edit connection form, shown in a dialog. Modeled on Beekeeper's
//! connection screen: type, host, credentials, SSL, SSH tunnel, read-only,
//! then name + color, with Test / Save / Save & Connect.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, StyledExt as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{
    AppError, AppResult, ConnectionColor, ConnectionConfig, Engine, Secrets, SshAuth, SshConfig,
    SslMode, parse_url,
};
use savoia_tunnel::HostKeyPolicy;

use crate::data_sources::DataSources;
use crate::{runtime, session};

const SSH_AUTH: [&str; 3] = ["Password", "Private key", "SSH agent"];

type Choice = SelectState<SearchableVec<&'static str>>;

enum Status {
    Idle,
    Testing,
    Ok(String),
    Error(String),
    UnknownHost { host: String, fingerprint: String },
}

pub struct ConnectionForm {
    data_sources: Entity<DataSources>,
    /// Set when editing; keeps the id so saving updates in place.
    editing: Option<ConnectionConfig>,

    url: Entity<InputState>,
    engine: Entity<Choice>,
    ssl: Entity<Choice>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    database: Entity<InputState>,

    ssh_enabled: bool,
    ssh_host: Entity<InputState>,
    ssh_port: Entity<InputState>,
    ssh_user: Entity<InputState>,
    ssh_auth: Entity<Choice>,
    ssh_password: Entity<InputState>,
    ssh_key_path: Entity<InputState>,
    ssh_passphrase: Entity<InputState>,

    read_only: bool,
    save_password: bool,
    name: Entity<InputState>,
    color: Option<ConnectionColor>,
    status: Status,
    _subscriptions: Vec<Subscription>,
}

fn input(window: &mut Window, cx: &mut App, placeholder: &str, value: &str) -> Entity<InputState> {
    let (placeholder, value) = (placeholder.to_owned(), value.to_owned());
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn secret(window: &mut Window, cx: &mut App, value: Option<&str>) -> Entity<InputState> {
    let value = value.unwrap_or_default().to_owned();
    cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .default_value(value)
    })
}

fn choice(
    window: &mut Window,
    cx: &mut App,
    items: Vec<&'static str>,
    selected: usize,
) -> Entity<Choice> {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(items),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

fn selected(choice: &Entity<Choice>, cx: &App) -> usize {
    choice.read(cx).selected_index(cx).map_or(0, |ix| ix.row)
}

fn text(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().trim().to_owned()
}

fn optional(input: &Entity<InputState>, cx: &App) -> Option<String> {
    Some(input.read(cx).value().to_string()).filter(|v| !v.is_empty())
}

impl ConnectionForm {
    /// `existing` is the connection being edited and its current secrets.
    pub fn new(
        data_sources: Entity<DataSources>,
        existing: Option<(ConnectionConfig, Secrets)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (config, secrets) = existing.clone().unwrap_or_else(|| {
            let mut c = ConnectionConfig::new(Engine::Postgres);
            c.user = String::new();
            (c, Secrets::default())
        });
        let ssh = config.ssh.clone();
        let engine_ix = Engine::ALL
            .iter()
            .position(|e| *e == config.engine)
            .unwrap_or(0);
        let ssl_ix = SslMode::ALL
            .iter()
            .position(|m| *m == config.ssl)
            .unwrap_or(0);
        let (ssh_auth_ix, key_path) = match ssh.as_ref().map(|s| &s.auth) {
            Some(SshAuth::PrivateKey { path }) => (1, path.display().to_string()),
            Some(SshAuth::Agent) => (2, String::new()),
            _ => (0, String::new()),
        };

        let engine = choice(
            window,
            cx,
            Engine::ALL.iter().map(|e| e.label()).collect(),
            engine_ix,
        );
        let port = input(window, cx, "5432", &config.port.to_string());
        let subscriptions = vec![cx.subscribe_in(
            &engine,
            window,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, window, cx| {
                let SelectEvent::Confirm(_) = event;
                // Follow the engine's default port unless the user typed a custom one.
                let engine = this.engine_value(cx);
                let current = text(&this.port, cx);
                if Engine::ALL
                    .iter()
                    .any(|e| e.default_port().to_string() == current)
                    || current.is_empty()
                {
                    let port = engine.default_port().to_string();
                    this.port.update(cx, |p, cx| p.set_value(port, window, cx));
                }
            },
        )];

        Self {
            data_sources,
            editing: existing.map(|(c, _)| c),
            url: input(
                window,
                cx,
                "postgres://user:password@host:5432/database",
                "",
            ),
            engine,
            ssl: choice(
                window,
                cx,
                SslMode::ALL.iter().map(|m| m.label()).collect(),
                ssl_ix,
            ),
            host: input(window, cx, "localhost", &config.host),
            port,
            user: input(window, cx, "postgres", &config.user),
            password: secret(window, cx, secrets.password.as_deref()),
            database: input(
                window,
                cx,
                "optional",
                config.database.as_deref().unwrap_or_default(),
            ),
            ssh_enabled: ssh.is_some(),
            ssh_host: input(
                window,
                cx,
                "bastion.example.com",
                ssh.as_ref().map_or("", |s| &s.host),
            ),
            ssh_port: input(
                window,
                cx,
                "22",
                &ssh.as_ref().map_or(22, |s| s.port).to_string(),
            ),
            ssh_user: input(window, cx, "ubuntu", ssh.as_ref().map_or("", |s| &s.user)),
            ssh_auth: choice(window, cx, SSH_AUTH.to_vec(), ssh_auth_ix),
            ssh_password: secret(window, cx, secrets.ssh_password.as_deref()),
            ssh_key_path: input(window, cx, "~/.ssh/id_ed25519", &key_path),
            ssh_passphrase: secret(window, cx, secrets.ssh_key_passphrase.as_deref()),
            read_only: config.read_only,
            save_password: config.save_password,
            name: input(window, cx, "Connection name (optional)", &config.name),
            color: config.color,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    fn engine_value(&self, cx: &App) -> Engine {
        Engine::ALL[selected(&self.engine, cx).min(Engine::ALL.len() - 1)]
    }

    /// Reads the form into a config and secrets, validating as it goes.
    fn collect(&self, cx: &App) -> AppResult<(ConnectionConfig, Secrets)> {
        let mut config = self
            .editing
            .clone()
            .unwrap_or_else(|| ConnectionConfig::new(self.engine_value(cx)));
        config.engine = self.engine_value(cx);
        config.name = text(&self.name, cx);
        config.color = self.color;
        config.host = text(&self.host, cx);
        config.port = text(&self.port, cx)
            .parse()
            .map_err(|_| AppError::invalid("Port must be a number"))?;
        config.user = text(&self.user, cx);
        config.database = Some(text(&self.database, cx)).filter(|d| !d.is_empty());
        config.ssl = SslMode::ALL[selected(&self.ssl, cx).min(SslMode::ALL.len() - 1)];
        config.read_only = self.read_only;
        config.save_password = self.save_password;
        config.ssh = if self.ssh_enabled {
            let auth = match selected(&self.ssh_auth, cx) {
                1 => SshAuth::PrivateKey {
                    path: expand_home(&text(&self.ssh_key_path, cx)).into(),
                },
                2 => SshAuth::Agent,
                _ => SshAuth::Password,
            };
            Some(SshConfig {
                host: text(&self.ssh_host, cx),
                port: text(&self.ssh_port, cx)
                    .parse()
                    .map_err(|_| AppError::invalid("SSH port must be a number"))?,
                user: text(&self.ssh_user, cx),
                auth,
            })
        } else {
            None
        };
        config.validate()?;

        let secrets = Secrets {
            password: optional(&self.password, cx),
            ssh_password: optional(&self.ssh_password, cx).filter(|_| self.ssh_enabled),
            ssh_key_passphrase: optional(&self.ssh_passphrase, cx).filter(|_| self.ssh_enabled),
        };
        Ok((config, secrets))
    }

    fn import_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = text(&self.url, cx);
        match parse_url(&url) {
            Ok((config, secrets)) => {
                let engine_ix = Engine::ALL
                    .iter()
                    .position(|e| *e == config.engine)
                    .unwrap_or(0);
                let ssl_ix = SslMode::ALL
                    .iter()
                    .position(|m| *m == config.ssl)
                    .unwrap_or(0);
                self.engine.update(cx, |s, cx| {
                    s.set_selected_index(Some(IndexPath::new(engine_ix)), window, cx)
                });
                self.ssl.update(cx, |s, cx| {
                    s.set_selected_index(Some(IndexPath::new(ssl_ix)), window, cx)
                });
                for (field, value) in [
                    (&self.host, config.host),
                    (&self.port, config.port.to_string()),
                    (&self.user, config.user),
                    (&self.database, config.database.unwrap_or_default()),
                    (&self.password, secrets.password.unwrap_or_default()),
                ] {
                    field.update(cx, |f, cx| f.set_value(value, window, cx));
                }
                self.url.update(cx, |f, cx| f.set_value("", window, cx));
                self.status = Status::Idle;
            }
            Err(err) => self.status = Status::Error(err.to_string()),
        }
        cx.notify();
    }

    fn test(&mut self, policy: HostKeyPolicy, cx: &mut Context<Self>) {
        let (config, secrets) = match self.collect(cx) {
            Ok(v) => v,
            Err(err) => {
                self.status = Status::Error(err.to_string());
                cx.notify();
                return;
            }
        };
        self.status = Status::Testing;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result =
                session::join(runtime::spawn(session::test(config, secrets, policy))).await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(info) => Status::Ok(format!("Connected to {}", info.version)),
                    Err(AppError::UnknownHostKey { host, fingerprint }) => {
                        Status::UnknownHost { host, fingerprint }
                    }
                    Err(err) => Status::Error(err.to_string()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save(&mut self, connect: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.collect(cx).and_then(|(config, secrets)| {
            let id = config.id;
            self.data_sources.update(cx, |ds, cx| {
                ds.save(config, secrets, cx)?;
                if connect {
                    ds.connect(id, HostKeyPolicy::KnownOnly, cx);
                }
                Ok(())
            })
        });
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(err) => {
                self.status = Status::Error(err.to_string());
                cx.notify();
            }
        }
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        match &self.status {
            Status::Idle => div().into_any_element(),
            Status::Testing => div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Testing…")
                .into_any_element(),
            Status::Ok(msg) => div()
                .text_sm()
                .text_color(theme.success)
                .child(msg.clone())
                .into_any_element(),
            Status::Error(msg) => div()
                .text_sm()
                .text_color(theme.danger)
                .child(msg.clone())
                .into_any_element(),
            Status::UnknownHost { host, fingerprint } => {
                v_flex()
                    .gap_1()
                    .text_sm()
                    .child(
                        div()
                            .text_color(theme.warning)
                            .child(format!("Unknown SSH host {host}")),
                    )
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("Fingerprint: {fingerprint}")),
                    )
                    .child(
                        Button::new("trust-host")
                            .small()
                            .label("Trust host and test")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.test(HostKeyPolicy::TrustUnknown, cx)
                            })),
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
impl ConnectionForm {
    pub fn url_input(&self) -> Entity<InputState> {
        self.url.clone()
    }

    pub fn status_text(&self) -> Option<String> {
        match &self.status {
            Status::Ok(m) | Status::Error(m) => Some(m.clone()),
            _ => None,
        }
    }
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => format!("{}/{rest}", home.to_string_lossy()),
        _ => path.to_owned(),
    }
}

fn field(label: &'static str, control: impl IntoElement) -> Div {
    v_flex()
        .gap_1()
        .flex_1()
        .child(div().text_xs().font_medium().child(label))
        .child(control)
}

fn section(title: &'static str, cx: &App) -> Div {
    div()
        .pt_2()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

impl Render for ConnectionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let ssh_auth = selected(&self.ssh_auth, cx);

        let swatches = h_flex().gap_1p5().children(
            std::iter::once(None)
                .chain(ConnectionColor::ALL.into_iter().map(Some))
                .map(|color| {
                    let chosen = self.color == color;
                    div()
                        .id(SharedString::from(format!("color-{color:?}")))
                        .size(px(16.))
                        .rounded_full()
                        .border_2()
                        .border_color(if chosen {
                            theme.foreground
                        } else {
                            theme.border
                        })
                        .when_some(color, |this, c| this.bg(rgb(c.rgb())))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.color = color;
                            cx.notify();
                        }))
                }),
        );

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_end()
                    .child(field("Import from URL", Input::new(&self.url).small()))
                    .child(
                        Button::new("import").small().label("Import").on_click(
                            cx.listener(|this, _, window, cx| this.import_url(window, cx)),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(field("Connection type", Select::new(&self.engine).small()))
                    .child(field("SSL mode", Select::new(&self.ssl).small())),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(field("Host", Input::new(&self.host).small()))
                    .child(
                        div()
                            .w(px(110.))
                            .child(field("Port", Input::new(&self.port).small())),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(field("User", Input::new(&self.user).small()))
                    .child(field(
                        "Password",
                        Input::new(&self.password).small().mask_toggle(),
                    )),
            )
            .child(field(
                "Default database",
                Input::new(&self.database).small(),
            ))
            .child(
                h_flex()
                    .justify_between()
                    .child(section("SSH TUNNEL", cx))
                    .child(
                        Switch::new("ssh-enabled")
                            .checked(self.ssh_enabled)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.ssh_enabled = *checked;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.ssh_enabled, |this| {
                this.child(
                    h_flex()
                        .gap_3()
                        .child(field("SSH host", Input::new(&self.ssh_host).small()))
                        .child(
                            div()
                                .w(px(110.))
                                .child(field("SSH port", Input::new(&self.ssh_port).small())),
                        ),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .child(field("SSH user", Input::new(&self.ssh_user).small()))
                        .child(field("Authentication", Select::new(&self.ssh_auth).small())),
                )
                .map(|this| match ssh_auth {
                    0 => this.child(field(
                        "SSH password",
                        Input::new(&self.ssh_password).small().mask_toggle(),
                    )),
                    1 => this.child(
                        h_flex()
                            .gap_3()
                            .child(field(
                                "Private key file",
                                Input::new(&self.ssh_key_path).small(),
                            ))
                            .child(field(
                                "Key passphrase",
                                Input::new(&self.ssh_passphrase).small().mask_toggle(),
                            )),
                    ),
                    _ => this.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Uses keys from the running ssh-agent (SSH_AUTH_SOCK)."),
                    ),
                })
            })
            .child(
                h_flex()
                    .gap_4()
                    .pt_1()
                    .child(
                        Checkbox::new("read-only")
                            .label("Read-only")
                            .checked(self.read_only)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.read_only = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new("save-password")
                            .label("Save passwords in the OS keychain")
                            .checked(self.save_password)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.save_password = *checked;
                                cx.notify();
                            })),
                    ),
            )
            .child(section("SAVE AS", cx))
            .child(
                h_flex()
                    .gap_3()
                    .child(div().flex_1().child(Input::new(&self.name).small()))
                    .child(swatches),
            )
            .child(self.render_status(cx))
            .child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("test")
                            .small()
                            .label("Test")
                            .disabled(matches!(self.status, Status::Testing))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.test(HostKeyPolicy::KnownOnly, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("cancel")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save").small().label("Save").on_click(
                            cx.listener(|this, _, window, cx| this.save(false, window, cx)),
                        ),
                    )
                    .child(
                        Button::new("save-connect")
                            .small()
                            .primary()
                            .label("Save & Connect")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save(true, window, cx)),
                            ),
                    ),
            )
    }
}
