// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use dbus::channel::MatchingReceiver;
use dbus::message::MatchRule;
use dbus::MethodErr;
use dbus_crossroads::Crossroads;
use dbus_crossroads::IfaceBuilder;
use hps_interface::Hps;
use hps_util::query_status;
use mcu_common::registers;

//
// EXAMPLE usage:
// dbus-send --session --dest=org.chromium.Hpsd --type=method_call --print-reply / org.chromium.Hpsd.query_status
// dbus-send --session --dest=org.chromium.Hpsd --type=method_call --print-reply / org.chromium.Hpsd.cam_inference
//
static HPS_SERVICE_NAME: &str = "org.chromium.Hpsd";
static HPS_INTERFACE_NAME: &str = "org.chromium.Hpsd";
static HPS_SERVICE_PATH: &str = "/org/chromium/Hpsd";

static DBUS_HW_VERSION_METHOD_NAME: &str = "hardware_version";
static DBUS_QUERY_STATUS_METHOD_NAME: &str = "query_status";
static DBUS_APP_VERSION_METHOD_NAME: &str = "application_version";
static DBUS_CAM_INFERENCE_METHOD_NAME: &str = "cam_inference";
static DBUS_SYS_ERRORS_METHOD_NAME: &str = "errors";

pub fn setup_ipc(hps: Box<dyn Hps>) -> Result<(), dbus::Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(handle_dbus(hps))
}

async fn handle_dbus(hps: Box<dyn Hps>) -> Result<(), dbus::Error> {
    let (lost, conn) = dbus_tokio::connection::new_session_sync()?;

    tokio::spawn(async {
        let error = lost.await;
        panic!("D-Bus connection lost: {}", error);
    });

    conn.request_name(HPS_SERVICE_NAME, false, true, false)
        .await?;

    let mut cr = Crossroads::new();

    let iface_token = cr.register(
        HPS_INTERFACE_NAME,
        |builder: &mut IfaceBuilder<Box<dyn Hps>>| {
            expose_register_as_string(
                builder,
                DBUS_HW_VERSION_METHOD_NAME,
                registers::HARDWARE_VERSION,
            );
            expose_register_as_string(
                builder,
                DBUS_APP_VERSION_METHOD_NAME,
                registers::APPLICATION_VERSION,
            );
            expose_register_as_string(
                builder,
                DBUS_CAM_INFERENCE_METHOD_NAME,
                registers::USER_PRESENT_STATUS,
            );
            expose_register_as_string(builder, DBUS_SYS_ERRORS_METHOD_NAME, registers::ERRORS);

            builder.method(
                DBUS_QUERY_STATUS_METHOD_NAME,
                (),
                ("reply",),
                |_ctx, hps, ()| convert_error(query_status(&mut **hps).map(|status| (status,))),
            );
        },
    );

    cr.insert(HPS_SERVICE_PATH, &[iface_token], hps);

    conn.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |message, conn| {
            cr.handle_message(message, conn).unwrap();
            true
        }),
    );

    futures::future::pending::<()>().await;
    unreachable!();
}

/// Exposes a register, converting the raw value to a string.
fn expose_register_as_string(
    builder: &mut IfaceBuilder<Box<dyn Hps>>,
    name: &'static str,
    register: u8,
) {
    builder.method(name, (), ("reply",), move |_ctx, hps, ()| {
        convert_error(hps.read_register(register).map(|val| (val.to_string(),)))
    });
}

fn convert_error<T>(result: Result<T, anyhow::Error>) -> Result<T, MethodErr> {
    result.map_err(|e| MethodErr::failed(&e.to_string()))
}
