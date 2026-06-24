pub struct BatteryStatus {
    pub percent:   u8,
    pub ac_online: bool,
}

pub struct SessionClient;

impl SessionClient {
    pub fn new() -> Self { Self }
    pub fn is_connected(&mut self) -> bool { false }
    pub fn get_battery(&mut self) -> Option<BatteryStatus> { None }
}
