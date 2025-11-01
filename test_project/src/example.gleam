import gleam/option.{type Option}

// gloss!: encoder(json), decoder
pub type Voice {
  Voice(
    // gloss!: maybe_absent
    mime_type: Option(String),
    // file_size by default is required but can be null
    file_size: Option(Int),
  )
}

// gloss!: encoder(json), decoder, camelCase
pub type User {
  User(
    user_name: String,
    user_age: Int,
    // gloss!: maybe_absent
    user_email: Option(String),
  )
}

// gloss!: encoder(json), decoder
pub type Status {
  Active
  Inactive
  Pending
}

// gloss!: encoder(json), decoder, type_tag = "kind"
pub type Message {
  Text(content: String)
  Image(url: String, width: Int, height: Int)
  Video(url: String, duration: Int)
}

// gloss!: encoder(json), decoder
pub type Product {
  Product(
    // gloss!: rename = "productName"
    name: String,
    // gloss!: rename = "productPrice"
    price: Float,
  )
}
