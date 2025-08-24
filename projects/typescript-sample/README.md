# TypeScript Sample Project

A simple TypeScript project that demonstrates basic TypeScript features including interfaces, classes, and type safety.

## Features

- TypeScript compilation with strict mode
- Simple User management system
- Source maps and declaration files
- Watch mode for development

## Setup

1. Install dependencies:
   ```bash
   yarn install
   ```

2. Build the project:
   ```bash
   yarn build
   ```

3. Run the compiled code:
   ```bash
   yarn start
   ```

## Development

For development with watch mode:
```bash
yarn dev
```

This will automatically recompile TypeScript files when they change.

## Project Structure

```
src/
├── main.ts          # Main application entry point
├── package.json     # Project dependencies and scripts
├── tsconfig.json    # TypeScript configuration
└── .gitignore       # Git ignore rules
```

## Build Output

The compiled JavaScript files are output to the `dist/` directory, which is gitignored.
