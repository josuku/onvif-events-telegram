use url::Url;

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct DiscoveryDevice {
    /// The WS-Discovery UUID / address reference
    pub address: String,
    pub name: Option<String>,
    pub urls: Vec<Url>,
}
