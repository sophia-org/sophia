use super::super::prelude::arg_value;

pub(super) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.first().map(String::as_str) == Some("sophia-shell-gpu-proof-exec") {
        let client = arg_value(args, "--client").ok_or("proof child requires --client")?;
        sophia_session::exec_shell_gpu_proof_client(std::path::Path::new(&client))?;
        return Ok(true);
    }
    if args
        .iter()
        .any(|arg| arg == "sophia-shell-gpu-content-hardware-proof")
    {
        if std::env::var_os("SOPHIA_LOM_GPU_PROOF_ARM").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            return Err("set SOPHIA_LOM_GPU_PROOF_ARM=1 to run the Lom GPU proof".into());
        }
        let client = arg_value(args, "--client").ok_or("proof requires --client=/absolute/path")?;
        let config = arg_value(args, "--config").ok_or("proof requires --config=/absolute/path")?;
        let seat = arg_value(args, "--seat").unwrap_or_else(|| "seat0".into());
        let render_node =
            arg_value(args, "--render-node").unwrap_or_else(|| "/dev/dri/renderD128".into());
        sophia_session::run_shell_gpu_content_hardware_proof(
            std::path::Path::new(&client),
            std::path::Path::new(&config),
            &seat,
            std::path::Path::new(&render_node),
        )?;
        return Ok(true);
    }

    Ok(false)
}
