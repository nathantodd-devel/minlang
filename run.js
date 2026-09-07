import { main } from ".";

main()
  .catch((e) => {
    console.error(e);
    process.exit(1);
  })
  .then((exit) => {
    process.exit(exit);
  });
