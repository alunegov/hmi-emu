use core::time;
use miniserde::{json, Deserialize};
use std::{env, fs, rc::Rc, sync::mpsc, thread::sleep, time::Instant};
use slint::{Model, ModelRc, VecModel};
use tokio_modbus::prelude::*;

#[derive(Deserialize, Debug)]
struct ParamSpec {
    id: u16,
    name: String,
    type_: i32,
}

slint::include_modules!();

fn main() {
    let mut args = env::args();
    let socket_addr = args.nth(1).unwrap_or_else(|| "192.168.50.230:1313".into()).parse().unwrap();

    let specs_json = fs::read_to_string("specs.json").unwrap_or_else(|_| "[]".to_string());
    let params_spec = json::from_str::<Vec<ParamSpec>>(&specs_json).unwrap();

    let ui = AppWindow::new().unwrap();
    let ui_weak = ui.as_weak();

    let (tx2, rx2) = mpsc::channel();

    let (tx1, rx1) = mpsc::channel();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.spawn(async move {
        'conn: loop {
            sleep(time::Duration::from_millis(1000));

            let mut ctx = match tcp::connect(socket_addr).await {
                Ok(ctx) => { println!("Connected"); ctx },
                Err(err) => { println!("Conn0, {err:?}"); continue 'conn; },
            };

            'read: loop {
                {
                    //let now = Instant::now();

                    let mut params: Vec<(i32, String, i32, Vec<bool>, f32)> = vec![];
                    for it in &params_spec {
                        let rsp = match ctx.read_input_registers((it.id - 1) * 2, 2).await {
                            Ok(rsp) => match rsp {
                                Ok(rsp) => rsp,
                                Err(err) => { println!("Exc1, {err:?}"); vec![0u16, 0u16] },
                            },
                            Err(err) => { println!("Conn1, {err:?}"); break 'read; },
                        };
                        //println!("{} value is: {rsp:?}", it.0);

                        match it.type_ {
                            0 => {
                                let param = to_int(rsp[0], rsp[1]);
                                //println!("{} value is: 0b{param:b}", it.0);

                                let mut bits = vec![false; 32];
                                for i in 0..32 {
                                    bits[i] = (param & (1u32 << i)) != 0;
                                }
            
                                params.push((it.id.into(), it.name.clone(), it.type_, bits, 0.0));
                            }
                            1 => {
                                let param = to_float(rsp[0], rsp[1]);
                                //println!("{} value is: {param}", it.0);

                                params.push((it.id.into(), it.name.clone(), it.type_, vec![], param));
                            }
                            _ => unreachable!()
                        }
                    }

                    //println!("read {}x time {:?}", params_spec.len(), now.elapsed());

                    let ui_copy = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let ui = ui_copy.unwrap();
                        let params_model: Rc<VecModel<Param>> = Default::default();
                        for it in params {
                            params_model.push(Param{
                                id: it.0,
                                text: it.1.into(),
                                type_: it.2,
                                val0: ModelRc::from(Rc::new(VecModel::from(it.3))),
                                val1: it.4
                            });
                        }
                        ui.set_params(ModelRc::from(params_model));
                    });
                }

                {
                    if let Ok((id, type_)) = rx2.try_recv() {
                        let now = Instant::now();
                        let rsp = match ctx.read_input_registers(((id - 1) * 2) as u16, 2).await {
                            Ok(rsp) => match rsp {
                                Ok(rsp) => rsp,
                                Err(err) => { println!("Exc2, {err:?}"); vec![0u16, 0u16] },
                            }
                            Err(err) => { println!("Conn2, {err:?}"); break 'read; }
                        };
                        println!("read time {:?}", now.elapsed());
                        //println!("{} value is: {rsp:?}", id);

                        let uni_value = match type_ {
                            0 => to_float(rsp[0], rsp[1]).to_string(),
                            1 => to_int(rsp[0], rsp[1]).to_string(),
                            _ => unreachable!()
                        };

                        let ui_copy = ui_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let ui = ui_copy.unwrap();
                            ui.set_uni_value(uni_value.into());
                        });
                    }
                }

                {
                    if let Ok((id, req0, req1)) = rx1.try_recv() {
                        let req = [req0, req1];
                        println!("{id} new value is: {req:?}");
                        let now = Instant::now();
                        match ctx.write_multiple_registers(((id - 1) * 2) as u16, &req).await {
                            Ok(rsp) => match rsp {
                                Ok(rsp) => rsp,
                                Err(err) => println!("Exc3, {err:?}"),
                            },
                            Err(err) => { println!("Conn3, {err:?}"); break 'read; },
                        }
                        println!("write time {:?}", now.elapsed());
                    }
                }

                sleep(time::Duration::from_millis(500));
            }

            // TODO: clear ui

            match ctx.disconnect().await {
                Ok(res) => match res {
                    Ok(_) => println!("Disconnected"),
                    Err(err) => println!("Exc4, {err:?}"),
                },
                Err(err) => println!("Conn4, {err:?}"),
            };
        }
    });

    ui.on_uni_load_clicked(move |id, type_| {
        let id = id.parse::<i32>().unwrap();
        tx2.send((id, type_)).unwrap();
    });

    let tx13 = tx1.clone();
    ui.on_uni_save_clicked(move |id, type_, value| {
        let id = id.parse::<i32>().unwrap();
        let req = match type_ {
            0 => {
                let val = value.parse::<f32>().unwrap();
                from_float(val)
            }
            1 => {
                let val = value.parse::<u32>().unwrap();
                from_int(val)
            }
            _ => unreachable!()
        };
        tx13.send((id, req[0], req[1])).unwrap();
    });

    let tx11 = tx1.clone();
    ui.on_flag_clicked(move |id, flags| {
        let mut val = 0u32;
        for i in 0..32 {
            let mask: u32 = 1u32 << i;
            if flags.row_data(i).unwrap() {
                val |= mask;
            } else {
                val &= !mask;
            }
        }
        let req = from_int(val);
        tx11.send((id, req[0], req[1])).unwrap();
    });

    ui.run().unwrap();
}

fn to_int(lo: u16, hi: u16) -> u32 {
    (lo as u32) | ((hi as u32) << 16)
}

fn from_int(val: u32) -> [u16; 2] {
    [val as u16, (val >> 16) as u16]
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

fn from_float(val: f32) -> [u16; 2] {
    let b = val.to_be_bytes();
    return [u16::from_be_bytes([b[2], b[3]]), u16::from_be_bytes([b[0], b[1]])];
}