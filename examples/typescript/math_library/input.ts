class Vector3 {
  public x: number;
  public y: number;
  public z: number;

  constructor(x: number, y: number, z: number) {
    this.x = x;
    this.y = y;
    this.z = z;
  }

  add(other: Vector3): Vector3 {
    return new Vector3(this.x + other.x, this.y + other.y, this.z + other.z);
  }

  sub(other: Vector3): Vector3 {
    return new Vector3(this.x - other.x, this.y - other.y, this.z - other.z);
  }

  dot(other: Vector3): number {
    return (this.x * other.x) + (this.y * other.y) + (this.z * other.z);
  }

  scale(scalar: number): Vector3 {
    return new Vector3(this.x * scalar, this.y * scalar, this.z * scalar);
  }

  magnitudeSquared(): number {
    return (this.x * this.x) + (this.y * this.y) + (this.z * this.z);
  }
}

const v1 = new Vector3(1, 2, 3);
const v2 = new Vector3(4, 5, 6);

const v3 = v1.add(v2);
console.log(v3.x);
console.log(v3.y);
console.log(v3.z);

const dotProd = v1.dot(v2);
console.log(dotProd);

const v1Scaled = v1.scale(2);
console.log(v1Scaled.x);
console.log(v1Scaled.y);
console.log(v1Scaled.z);

const magSq = v1.magnitudeSquared();
console.log(magSq);
