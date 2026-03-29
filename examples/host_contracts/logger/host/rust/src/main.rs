mod generated;

use generated::host::host_contracts::HostLogger;
use generated::host::vtable_factories::create_host_logger_vtable;
use polyplug::runtime::Runtime;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[PLUGIN LOG] {}", message);
    }
}

fn main() -> Result<(), String> {
    let runtime: Runtime = Runtime::builder().build().map_err(|e| format!("{:?}", e))?;

    let vtable = create_host_logger_vtable(Box::new(ConsoleLogger));
    runtime
        .register_host_contract(0xF53EB5F2845853BB, vtable)
        .map_err(|e| format!("{:?}", e))?;

    println!("Logger host contract registered successfully!");
    Ok(())
}
