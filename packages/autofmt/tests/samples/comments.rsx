rsx! {
    div {
        // Comments
        class: "asdasd",
        "hello world"
    }
    div {
        // My comment here 1
        // My comment here 2
        // My comment here 3
        // My comment here 4
        class: "asdasd",

        // Comment here
        onclick: move |_| {
            let a = 10;
            let b = 40;
            let c = 50;
        },

        // my comment

        // This here
        "hi"
    }

    // Comment head
    div { class: "asd", "Jon" }

    // Comment head
    div {
        // Collapse
        class: "asd",
        "Jon"
    }

    // comments inline
    div { // inline
        // Collapse
        class: "asd", // super inline
        class: "asd", // super inline
        "Jon" // all the inline
        // Comments at the end too
    }

    // please dont eat me 1
    div { // please dont eat me 2
        // please dont eat me 3
    }

    // please dont eat me 1
    div { // please dont eat me 2
        // please dont eat me 3
        abc: 123,
    }

    // please dont eat me 1
    div {
        // please dont eat me 3
        abc: 123,
    }

    div {
        // I am just a comment
    }

    div {
        "text"
        // I am just a comment
    }

    div {
        div {}
        // I am just a comment
    }

    div {
        {some_expr()}
        // I am just a comment
    }

    div {
        "text" // I am just a comment
    }

    div {
        div {} // I am just a comment
    }

    div {
        {some_expr()} // I am just a comment
    }

    div {
        // Please dont eat me 1
        div {
            // Please dont eat me 2
        }
        // Please dont eat me 3
    }

    div {
        "hi"
        // Please dont eat me 1
    }
    div {
        "hi" // Please dont eat me 1
        // Please dont eat me 2
    }

    // Please dont eat me 2
    Component {}

    // Please dont eat me 1
    Component {
        // Please dont eat me 2
    }

    // Please dont eat me 1
    Component {
        // Please dont eat me 2
    }

    // Please dont eat me 1
    //
    // Please dont eat me 2
}
