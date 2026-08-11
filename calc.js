class Point {
  constructor(fields) {
    this["x"] = fields && fields["x"] !== undefined ? fields["x"] : 0;
    this["y"] = fields && fields["y"] !== undefined ? fields["y"] : 0;
  }
}

const Shape_Circle = "Shape.Circle";
const Shape_Square = "Shape.Square";
const Shape_Triangle = "Shape.Triangle";

function area(shape, size) {
  return (() => {
  switch (shape) {
      case Shape_Circle:
        return ((3.14159 * size) * size);
      case Shape_Square:
        return (size * size);
      case Shape_Triangle:
        return ((0.5 * size) * size);
    default:
      throw new Error("match 未穷尽");
  }
})();
}

function distance(p, q) {
  let dx = (p.x - q.x);
  let dy = (p.y - q.y);
  return ((dx * dx) + (dy * dy));
}

function main() {
  console.log(["圆面积:", area(Shape_Circle, 2)].map(String).join(" "));
  console.log(["方面积:", area(Shape_Square, 3)].map(String).join(" "));
  console.log(["三角面积:", area(Shape_Triangle, 4)].map(String).join(" "));
  let p = new Point({ "x": 0, "y": 0 });
  let q = new Point({ "x": 3, "y": 4 });
  console.log(["距离平方:", distance(p, q)].map(String).join(" "));
  let s = Shape_Triangle;
  if ((s === Shape_Triangle)) {
  console.log(["枚举比较: 是三角形"].map(String).join(" "));
  }
}

main();
