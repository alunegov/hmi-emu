use core::time;
use std::{env, sync::mpsc, thread::sleep};
use tokio_modbus::prelude::*;

slint::include_modules!();

fn main() {
    let mut args = env::args();
    let socket_addr = args.nth(1).unwrap_or_else(|| "192.168.50.230:502".into()).parse().unwrap();

    let ui = AppWindow::new().unwrap();
    let ui_weak = ui.as_weak();

    let (tx, rx) = mpsc::channel();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.spawn(async move {
        let mut ctx = tcp::connect(socket_addr).await.unwrap();

        let mut p273: u32;

        // read p273
        {
            let rsp =  ctx.read_input_registers(544, 2).await.unwrap().unwrap();
            println!("r544 value is: {rsp:?}");

            p273 = (rsp[0] as u32) | ((rsp[1] as u32) << 8);

            let ui_copy = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let ui = ui_copy.unwrap();
                ui.set_checked1((p273 & (1u32 << 0)) != 0);
                ui.set_checked2((p273 & (1u32 << 1)) != 0);
                ui.set_checked3((p273 & (1u32 << 2)) != 0);
                ui.set_checked4((p273 & (1u32 << 3)) != 0);
                ui.set_checked5((p273 & (1u32 << 4)) != 0);
                ui.set_checked6((p273 & (1u32 << 5)) != 0);
            });
        }

        loop {
            // ai
            {
                let rsp = ctx.read_input_registers(80, 12).await.unwrap().unwrap();
                //println!("r80 value is: {rsp:?}");

                let ai1 = to_float(rsp[2], rsp[3]);
                let ai2 = to_float(rsp[6], rsp[7]);
                let ai3 = to_float(rsp[10], rsp[11]);

                let ui_copy = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let ui = ui_copy.unwrap();
                    ui.set_ai1(ai1);
                    ui.set_ai2(ai2);
                    ui.set_ai3(ai3);
                });
            }

            // ao
            {
                let rsp =  ctx.read_input_registers(464, 12).await.unwrap().unwrap();
                //println!("r464 value is: {rsp:?}");

                let ao1 = to_float(rsp[2], rsp[3]);
                let ao2 = to_float(rsp[6], rsp[7]);
                let ao3 = to_float(rsp[10], rsp[11]);

                let ui_copy = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let ui = ui_copy.unwrap();
                    ui.set_ao1(ao1);
                    ui.set_ao2(ao2);
                    ui.set_ao3(ao3);
                });
            }

            // write p273
            {
                if let Ok((id, checked)) = rx.try_recv() {
                    let mask: u32 = 1u32 << id;
                    if checked {
                        p273 |= mask;
                    } else {
                        p273 &= !mask;
                    }
                    let req = [p273 as u16, (p273 >> 8) as u16];
                    println!("r544 new value is: {req:?}");
                    ctx.write_multiple_registers(544, &req).await.unwrap().unwrap();
                }
            }

            sleep(time::Duration::from_millis(500));
        }

        //println!("Disconnecting");
        //ctx.disconnect().await.unwrap();
    });

    ui.on_cb_checked(move |id, checked| {
        //println!("checkbox {id} {checked}");
        tx.send((id, checked)).unwrap();
    });

    ui.run().unwrap();
}

fn to_float(lo: u16, hi: u16) -> f32 {
    let mut b = [0u8; 4];
    let [h, l] = hi.to_be_bytes();
    b[0] = h;
    b[1] = l;
    let [h, l] = lo.to_be_bytes();
    b[2] = h;
    b[3] = l;
    f32::from_be_bytes(b)
}
