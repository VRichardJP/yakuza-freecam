use memory_rs::process::process_wrapper::Process;
use winapi::um::winuser;
use winapi::um::winuser::{GetCursorPos, SetCursorPos, GetAsyncKeyState};
use winapi::shared::windef::{POINT};
use std::io::{Error, ErrorKind};
use std::thread;
use std::time::{Duration, Instant};
use std::f32;
use crate::common::{Camera, Injection};

const INITIAL_POS: i32 = 500;

#[naked]
unsafe fn shellcode() {
    llvm_asm!("
    push r11
    lea r11,[rip+0x200-0x9];
    pushf
    push rax
    mov eax, [r11-0x10]
    test eax, eax
    pop rax
    je not_zero
    movaps xmm4,[r11+0x40] // rotation
    movaps xmm10,[r11] // focus
    movaps xmm12,[r11+0x20] // position
    // FOV 
    push rax
    mov rax,[r11+0x60]
    mov [rdx+0x58],rax
    pop rax

not_zero:
    movaps [r11],xmm10
    movaps [r11+0x20],xmm12
    movaps [r11+0x40],xmm4 // camera rotation
    push rax
    mov rax,[rdx+0x58]
    mov [r11+0x60],rax
    pop rax

    popf
    pop r11
    subps xmm10,xmm12
    movq xmm0,rax
    ret
    nop;nop;nop;nop;
    ": : : : "volatile", "intel");
}

#[naked]
unsafe fn get_pause_value() {
    llvm_asm!("
    push rax
    lea rax,[rdi+0x188]
    mov [rip+0x200-0xF],rax
    pop rax

    // original code
    movzx eax,byte ptr [rdi+0x188]
    ret
    nop;nop;nop;nop;
    ": : : : "volatile", "intel");
}

fn trigger_pause(process: &Process, addr: usize) {
    if addr == 0x0 { return; }
    process.write_value::<u8>(addr, 0x1);
    thread::sleep(Duration::from_millis(20));
    process.write_value::<u8>(addr, 0x0);
}

pub fn main() -> Result<(), Error> {
    let mut mouse_pos: POINT = POINT::default();

    // latest mouse positions
    let mut latest_x = 0;
    let mut latest_y = 0;

    println!("
    INSTRUCTIONS:

    PAUSE - Activate/Deactivate Free Camera
    END - Deactivate UI
    DEL - Deattach Mouse

    UP, DOWN, LEFT, RIGHT - Move in the direction you're pointing
    CTRL, SPACE - Move UP or DOWN
    PG UP, PG DOWN - Increase/Decrease speed multiplier
    F1, F2 - Increase/Decrease FOV respectively

    WARNING: Don't forget to deactivate the freecam before skipping a cutscene
    (it may cause a game freeze)

    WARNING: Once you deattach the camera (PAUSE), your mouse will be set in a fixed
    position, so in order to attach/deattach the mouse to the camera, you can
    press DEL
    ");

    println!("Waiting for the game to start");
    let yakuza = loop {
        match Process::new("YakuzaKiwami2.exe") {
            Ok(p) => break p,
            Err(_) => (),
        }

        thread::sleep(Duration::from_secs(5));
    };
    println!("Game hooked");

    let entry_point: usize = 0x1F0222B;

    let p_shellcode = yakuza.inject_shellcode(entry_point, 9,
        shellcode as usize as *const u8);

    let mut cam = Camera::new(p_shellcode);

    // function that changes the focal length of the cinematics, when
    // active, nop this
    cam.injections.push(Injection {
        entry_point: 0xB78D87,
        f_orig: vec![0x89, 0x86, 0xB8, 0x00, 0x00, 0x00],
        f_rep: vec![0x90; 6] }
    );

    // nop the setcursorpos inside the game
    cam.injections.push(Injection {
        entry_point: 0x1BA285B,
        f_orig: vec![0xFF, 0x15, 0x47, 0x52, 0x4A, 0x00],
        f_rep: vec![0x90; 6]
    });

    // WIP: Pause the cinematics of the world.
    cam.injections.push(Injection {
        entry_point: 0xDF6F86,
        f_orig: vec![0x0F, 0x84, 0x5E, 0x02, 0x00, 0x00],
        f_rep: vec![0xE9, 0x5F, 0x02, 0x00, 0x00, 0x90]
    });

    // Pause world entry point
    let pause_value_ep: usize = 0xDF5E1B;
    let pause_value = yakuza.inject_shellcode(pause_value_ep, 7,
        get_pause_value as usize as *const u8);

    // Hide UI stuff
    cam.injections.push(Injection {
        entry_point: 0x8B2E8C,
        f_orig: vec![0x41, 0x0F, 0x29, 0x9E, 0x70, 0x01, 0x00, 0x00],
        f_rep: vec![0x45, 0x0F, 0x29, 0x8E, 0x70, 0x01, 0x00, 0x00]
    });

    // flashy health bar
    cam.injections.push(Injection {
        entry_point: 0x1B71453,
        f_orig: vec![0xC6, 0x04, 0x0B, 0x01],
        f_rep: vec![0xC6, 0x04, 0x0B, 0x00]
    });

    let mut active = false;
    let mut capture_mouse = false;

    let mut restart_mouse = false;

    loop {
        if capture_mouse & restart_mouse {
            unsafe { SetCursorPos(INITIAL_POS, INITIAL_POS) };
            restart_mouse = !restart_mouse;
            latest_x = INITIAL_POS;
            latest_y = INITIAL_POS;
            continue;
        }

        let start = Instant::now();

        // poll rate
        thread::sleep(Duration::from_millis(10));
        unsafe { GetCursorPos(&mut mouse_pos) };
        let duration = start.elapsed().as_millis() as f32;

        let speed_x = ((mouse_pos.x - latest_x) as f32)/duration/100.;
        let speed_y = ((mouse_pos.y - latest_y) as f32)/duration/100.;

        let c_v_a = yakuza.read_value::<usize>(pause_value+0x200);

        if active && capture_mouse {
            cam.update_position(&yakuza, speed_x, speed_y);
        }

        latest_x = mouse_pos.x;
        latest_y = mouse_pos.y;

        // to scroll infinitely
        restart_mouse = !restart_mouse;
        unsafe {
            if (GetAsyncKeyState(winuser::VK_PAUSE) as u32 & 0x8000) != 0 {
                active = !active;
                capture_mouse = active;

                let c_status = if active { "Deattached" } else { "Attached" };
                println!("status of camera: {}", c_status);

                if active {
                    cam.deattach(&yakuza);
                } else {
                    cam.attach(&yakuza);
                }

                trigger_pause(&yakuza, c_v_a);
                thread::sleep(Duration::from_millis(500));
            }

            if active &
                (GetAsyncKeyState(winuser::VK_DELETE) as u32 & 0x8000 != 0) {
                capture_mouse = !capture_mouse;
                let c_status = if !capture_mouse { "Deattached" } else { "Attached" };
                println!("status of mouse: {}", c_status);
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}
