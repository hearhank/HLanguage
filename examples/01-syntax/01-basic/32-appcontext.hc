// Test AppContext IoC container (ADR-0026)
import H.std.{io};

fn main() !void {
    // 1. Create AppContext
    var ctx = AppContext.init(alloc);
    defer ctx.deinit();

    // 2. Register a singleton
    ctx.register("IValue", 42);

    // 3. Get the singleton (returns pointer to 42)
    var v = ctx.get("IValue");
    io.print("v = {}\n", v);

    // 4. Named registration: register(type_name, name, impl)
    ctx.register("IValidator", "email", 100);
    ctx.register("IValidator", "phone", 200);

    // 5. Named get
    var email_v = ctx.get("IValidator", "email");
    var phone_v = ctx.get("IValidator", "phone");
    io.print("email = {}, phone = {}\n", email_v, phone_v);

    // 6. Register a factory (no-arg closure)
    ctx.registerFactory("make_answer", | | 42);

    // 7. Make a new instance from factory
    var x = ctx.make("make_answer");
    io.print("x = {}\n", x);
}