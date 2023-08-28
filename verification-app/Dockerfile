# Dockerfile

# Stage 1: Build the application
FROM node:18 as build

WORKDIR /app

COPY package.json yarn.lock ./

RUN yarn install

COPY . .

RUN yarn build

# Stage 2: Serve the application using a lightweight Node.js image
FROM node:18-alpine

WORKDIR /app

# Copy the build directory, the public directory, and the node_modules directory from the build stage
COPY --from=build /app/build ./build

RUN yarn global add serve

EXPOSE 3000

CMD ["serve", "-s", "build", "-l", "3000"]