# Callbacks into Rust




# Subclasses

There is limited and experimental support for creating Rust subclasses of
C++ classes. (Yes, even more experimental than all the rest of this!)
See [`subclass::CppSubclass`] for information about how you do this.
This is useful primarily if you want to listen out for messages broadcast
using the C++ observer/listener pattern.
