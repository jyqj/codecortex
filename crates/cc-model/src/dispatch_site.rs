use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchSiteKind {
    EventOn,
    EventEmit,
    CallbackStore,
    CallbackInvoke,
    JsxTag,
    StateSetterBinding,
    StateSetterCall,
}

impl DispatchSiteKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EventOn => "event_on",
            Self::EventEmit => "event_emit",
            Self::CallbackStore => "callback_store",
            Self::CallbackInvoke => "callback_invoke",
            Self::JsxTag => "jsx_tag",
            Self::StateSetterBinding => "state_setter_binding",
            Self::StateSetterCall => "state_setter_call",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "event_on" => Self::EventOn,
            "event_emit" => Self::EventEmit,
            "callback_store" => Self::CallbackStore,
            "callback_invoke" => Self::CallbackInvoke,
            "jsx_tag" => Self::JsxTag,
            "state_setter_binding" => Self::StateSetterBinding,
            "state_setter_call" => Self::StateSetterCall,
            _ => Self::EventOn,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchSiteRecord {
    pub site_id: String,
    pub file_path: String,
    pub line: u32,
    pub col: u32,
    pub enclosing_symbol_uid: Option<String>,
    pub receiver_expr: Option<String>,
    pub site_kind: DispatchSiteKind,
    pub key: String,
    pub handler_expr: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub confidence: f64,
}
