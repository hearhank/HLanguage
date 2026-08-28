fn main(args: Vec<String>) !void {
    if (args.len >= 99 and args[50].as_slice() == "x") {
        io.print("taken\n");
    }
    io.print("no short-circuit test done\n");
}
