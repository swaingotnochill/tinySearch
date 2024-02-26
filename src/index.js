console.log("query api search");
fetch("/api/search", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
  },
  body: JSON.stringify('{"query": "bind texture to buffer"}'),
}).then((response) => console.log(response));
