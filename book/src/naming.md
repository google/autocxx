# Naming

## Namespaces

The C++ namespace structure is reflected in mods within the generated
ffi mod. However, at present there is an internal limitation that
autocxx can't handle multiple types with the same identifier, even
if they're in different namespaces. This will be fixed in future.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"",
"
namespace submarines {
struct Boomer {
    int number_of_missiles;
};
}

namespace generations {
struct Boomer {
    bool ok;
};
}
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate_pod!("submarines::Boomer")
    generate_pod!("generations::Boomer")
}

fn main() {
    let best_possible_age_person_ = ffi::generations::Boomer {
        ok: true,
    };
    let submarine_ = ffi::submarines::Boomer {
        number_of_missiles: 12
    };
}
}
)
```

## Nested classes

There is support for generating bindings of nested types, with some
restrictions. Currently the C++ type `A::B` will be given the Rust name
`A_B` in the same module as its enclosing namespace.

## Overloads

See [the chapter on C++ functions](cpp_functions.md).
