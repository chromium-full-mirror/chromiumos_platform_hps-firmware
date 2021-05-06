use dbus::tree::MethodErr;
use dbus::{BusType, Connection, NameFlag};
use dbus_tokio::tree::{AFactory, ATree, ATreeServer};
use dbus_tokio::AConnection;
use futures::{future, Stream};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio_core::reactor::Core;

use mcu_common::registers;

use hps_util::query_status;
use hps_util::Ft2232hHps;

//
// EXAMPLE usage:
// dbus-send --session --dest=org.chromium.Hps --type=method_call --print-reply / org.chromium.Hps.query_status
// dbus-send --session --dest=org.chromium.Hps --type=method_call --print-reply / org.chromium.Hps.cam_inference
//
static HPS_SERVICE_NAME: &str = "org.chromium.Hps";
static HPS_INTERFACE_NAME: &str = "org.chromium.Hps";
static HPS_SERVICE_PATH: &str = "/org/chromium/Hps";

static DBUS_HW_VERSION_METHOD_NAME: &str = "hardware_version";
static DBUS_QUERY_STATUS_METHOD_NAME: &str = "query_status";
static DBUS_APP_VERSION_METHOD_NAME: &str = "application_version";
static DBUS_CAM_INFERENCE_METHOD_NAME: &str = "cam_inference";
static DBUS_SYS_ERRORS_METHOD_NAME: &str = "errors";

pub fn setup_ipc(hps: Arc<Mutex<Ft2232hHps>>) -> Result<(), dbus::Error> {
    let conn = Rc::new(Connection::get_private(BusType::Session).unwrap());
    conn.register_name(HPS_SERVICE_NAME, NameFlag::ReplaceExisting as u32)?;

    let f = AFactory::new_afn::<()>();

    let hw_version_method = f
        .amethod(DBUS_HW_VERSION_METHOD_NAME, (), {
            let hps = hps.clone();
            move |m| {
                let mret = m.msg.method_return();

                let reg_val = {
                    let hps: &mut Ft2232hHps = &mut hps.lock().unwrap();
                    hps.read_register(registers::HARDWARE_VERSION)
                };
                let reg_ret: String = match reg_val {
                    Ok(v) => v.to_string(),
                    Err(_) => return future::err(MethodErr::failed(&"read_reg failed".to_owned())),
                };
                let mret = mret.append1(reg_ret);

                let reply = vec![mret];
                future::ok(reply)
            }
        })
        .outarg::<&str, _>("reply");

    let query_status_method = f
        .amethod(DBUS_QUERY_STATUS_METHOD_NAME, (), {
            let hps = hps.clone();
            move |m| {
                let mret = m.msg.method_return();

                let err_str = query_status(&mut hps.lock().unwrap());
                let mret = mret.append1(err_str.unwrap());

                let reply = vec![mret];
                future::ok(reply)
            }
        })
        .outarg::<&str, _>("reply");

    let app_version_method = f
        .amethod(DBUS_APP_VERSION_METHOD_NAME, (), {
            let hps = hps.clone();
            move |m| {
                let mret = m.msg.method_return();

                let reg_val = {
                    let hps: &mut Ft2232hHps = &mut hps.lock().unwrap();
                    hps.read_register(registers::APPLICATION_VERSION)
                };
                let reg_ret: String = match reg_val {
                    Ok(v) => v.to_string(),
                    Err(_) => return future::err(MethodErr::failed(&"read_reg failed".to_owned())),
                };
                let mret = mret.append1(reg_ret);

                let reply = vec![mret];
                future::ok(reply)
            }
        })
        .outarg::<&str, _>("reply");

    let cam_inference_method = f
        .amethod(DBUS_CAM_INFERENCE_METHOD_NAME, (), {
            let hps = hps.clone();
            move |m| {
                let mret = m.msg.method_return();

                let reg_08 = {
                    let hps: &mut Ft2232hHps = &mut hps.lock().unwrap();
                    hps.read_register(registers::USER_PRESENT_STATUS)
                };
                let inference_ret: String = match reg_08 {
                    Ok(v) => v.to_string(),
                    Err(_) => {
                        return future::err(MethodErr::failed(
                            &"read_reg for 0x08 failed".to_owned(),
                        ))
                    }
                };
                let mret = mret.append1(inference_ret);

                let reply = vec![mret];
                future::ok(reply)
            }
        })
        .outarg::<&str, _>("reply");

    let hps_errors_method = f
        .amethod(DBUS_SYS_ERRORS_METHOD_NAME, (), {
            let hps = hps.clone();
            move |m| {
                let mret = m.msg.method_return();

                let reg_val = {
                    let hps: &mut Ft2232hHps = &mut hps.lock().unwrap();
                    hps.read_register(registers::ERRORS)
                };
                let reg_ret: String = match reg_val {
                    Ok(v) => v.to_string(),
                    Err(_) => return future::err(MethodErr::failed(&"read_reg failed".to_owned())),
                };
                let mret = mret.append1(reg_ret);

                let reply = vec![mret];
                future::ok(reply)
            }
        })
        .outarg::<&str, _>("reply");

    let dbus_intf = f
        .interface(HPS_INTERFACE_NAME, ())
        .add_m(hw_version_method)
        .add_m(query_status_method)
        .add_m(app_version_method)
        .add_m(cam_inference_method)
        .add_m(hps_errors_method);

    let dbus_obj = f
        .object_path(HPS_SERVICE_PATH, ())
        .introspectable()
        .add(dbus_intf);
    let tree = f.tree(ATree::new()).add(dbus_obj);

    tree.set_registered(&conn, true)?;

    let mut core = Core::new().unwrap();
    let aconn = AConnection::new(conn.clone(), core.handle()).unwrap();
    let server = ATreeServer::new(conn.clone(), &tree, aconn.messages().unwrap());

    let server = server.for_each(|_| Ok(()));
    core.run(server).unwrap();

    Ok(())
}
