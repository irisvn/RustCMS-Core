//! JSON routes of the panel's own pages, on its session (`AdminRoutes`,
//! under `/admin/api/x`).

use rustcms::prelude::*;

use crate::http::controllers::live_controller;

pub fn routes() -> AdminRoutes {
    AdminRoutes::new("/demo")
        .events("/ticks", AdminAccess::can("demo.live"), live_controller::ticks)
        .post("/ticks/reset", AdminAccess::can("demo.live"), live_controller::reset)
        .socket("/echo", AdminAccess::can("demo.live"), live_controller::echo)
        .get("/hello", AdminAccess::can("demo.live"), live_controller::hello)
}
