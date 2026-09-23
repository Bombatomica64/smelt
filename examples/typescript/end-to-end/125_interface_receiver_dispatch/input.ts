// A value typed by an INTERFACE dispatches to the class that implements it.
//
// `Shape` declares a readonly property and methods; two classes implement it,
// one with a plain field and one with a getter. Code that only knows the
// interface type (a parameter, a factory callback's result, a list element)
// reads the property through the class's own field or getter and calls the
// methods on the class's own `impl`, instead of reading them as fields of an
// erased record.

interface Shape {
  readonly name: string
  area(): number
  tag(label: string): string
}

class Square implements Shape {
  readonly name: string = 'square'
  tagged: number = 0
  private side: number
  constructor(side: number) {
    this.side = side
  }
  area(): number {
    return this.side * this.side
  }
  tag(label: string): string {
    this.tagged += 1
    return label
  }
}

class Circle implements Shape {
  private radius: number
  constructor(radius: number) {
    this.radius = radius
  }
  readonly name: string = 'circle'
  tagged: number = 0
  area(): number {
    return Math.round(Math.PI * this.radius * this.radius)
  }
  tag(label: string): string {
    this.tagged += 2
    return label
  }
}

function describe(shape: Shape): string {
  return `${shape.name} area=${shape.area()} ${shape.tag('x')}`
}

function runSuite({ make }: { make: () => Shape }): void {
  const shape = make()
  console.log(shape.name, shape.area(), shape.tag('suite'))
}

const square = new Square(3)
const shapes: Shape[] = [square, new Circle(2)]
for (const shape of shapes) {
  console.log(describe(shape))
}
console.log(describe(new Square(5)))
// The interface view dispatches on the SAME instance: its method's effect
// on the class's own state is visible through the class afterwards.
console.log('square tagged', square.tagged)
runSuite({ make: () => new Square(4) })
runSuite({ make: () => new Circle(1) })
