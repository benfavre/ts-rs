// @expect: no-errors
// @hover animal: Animal
// @hover dog: Dog
// @hover name: string
// @hover bark: string
//
// Class instance hover (test_new_class_* canonical form) — `new Animal()`
// hovers as the NOMINAL class name, not a structural ObjectType. Property
// access through inheritance still works; assert that by reading
// inherited / own properties below.

class Animal {
  species: string = "";
  move(): void {}
}

class Dog extends Animal {
  bark(): string { return "woof"; }
}

const animal = new Animal();
const dog = new Dog();

// Property access: inherited + own + return type.
const name: string = dog.species;
const bark: string = dog.bark();

export { animal, dog, name, bark };
