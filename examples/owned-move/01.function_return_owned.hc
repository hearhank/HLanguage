class AstNode {
    kind: &[u8],
    props: Vec<u8>,
    children: Vec<AstNode>,
}

fn make_node(kind: &[u8]) AstNode {
    var n = AstNode{
        kind = kind,
        props = Vec<u8>.init(alloc),
        children = Vec<AstNode>.init(alloc),
    };
    return n;
}

fn main(){
    var node=make_node("root");
    io.print("{}",node.kind);
}

[test]
fn test_owned_move_fn(){
     var node=make_node("root");
     io.print("{}",node.kind);
}
