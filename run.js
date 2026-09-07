import { main } from "./build/index.js";

main()
  .catch((e) => {
    console.error(e);
    process.exit(1);
  })
  .then((exit) => {
    process.exit(exit);
  });
