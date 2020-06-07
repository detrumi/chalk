use super::*;

#[test]
fn opaque_bounds() {
    test! {
        program {
            struct Ty { }

            trait Clone { }
            opaque type T: Clone = Ty;
        }

        goal {
            T: Clone
        } yields {
            "Unique; substitution []"
        }
    }
}

#[test]
fn opaque_reveal() {
    test! {
        program {
            struct Ty { }
            trait Trait { }
            impl Trait for Ty { }

            trait Clone { }
            opaque type T: Clone = Ty;
        }

        goal {
            if (Reveal) {
                T: Trait
            }
        } yields {
            "Unique; substitution []"
        }

        goal {
            T: Trait
        } yields {
            "No possible solution"
        }
    }
}

#[test]
fn opaque_where_clause() {
    test! {
        program {
            struct Ty { }

            trait Clone { }
            trait Trait { }
            impl Trait for Ty { }
            opaque type T: Clone where T: Trait = Ty;
        }

        goal {
            T: Trait
        } yields {
            "Unique; substitution []"
        }
    }
}

#[test]
fn opaque_generics_simple() {
    test! {
        program {
            trait Iterator { type Item; }

            struct Vec<T> { }
            struct Bar { }
            impl<T> Iterator for Vec<T> {
                type Item = u32;
            }

            opaque type Foo<X>: Iterator = Vec<X>;
        }

        goal {
            Foo<Bar>: Iterator
        } yields {
            "Unique; substitution []"
        }

    }
}

#[test]
fn opaque_generics() {
    test! {
        program {
            trait Iterator { type Item; }

            struct Vec<T> { }
            struct Bar { }

            opaque type Foo<X>: Iterator<Item = X> = Vec<X>;
        }

        goal {
            Foo<Bar>: Iterator<Item = Bar>
        } yields {
            "Unique; substitution []"
        }

        goal {
            forall<T> {
                Foo<T>: Iterator<Item = T>
            }
        } yields {
            "Unique; substitution []"
        }

        goal {
            exists<T> {
                <Foo<Bar> as Iterator>::Item = T
            }
        } yields[SolverChoice::slg_default()] {
            "Ambiguous" // #234
        } yields[SolverChoice::recursive()] {
            "Unique; substitution [?0 := Bar], lifetime constraints []"
        }
    }
}

#[test]
fn opaque_auto_traits() {
    test! {
        program {
            struct Bar { }
            struct Baz { }
            trait Trait { }

            #[auto]
            trait Send { }

            impl !Send for Baz { }

            opaque type Opaque1: Trait = Bar;
            opaque type Opaque2: Trait = Baz;
        }

        goal {
            Opaque1: Send
        } yields {
            "Unique"
        }

        goal {
            Opaque2: Send
        } yields {
            "No possible solution"
        }
    }
}
