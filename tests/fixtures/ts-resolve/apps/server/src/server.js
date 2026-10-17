const express = require("express");
const { list } = require("./routes");

const app = express();
app.get("/users", list);
app.listen(3000);
