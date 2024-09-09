# Build stage
FROM rust:1.80-slim AS builder

# Set the working directory
WORKDIR /app

# Copy the source code
COPY . .

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM ubuntu:22.04

RUN apt-get update && apt-get install -y curl libssl3 ca-certificates

# Set the working directory
WORKDIR /app

# Copy the binary from the build stage
COPY --from=builder /app/target/release/enclave-nitro-demo .

# Expose port 8080
EXPOSE 8080

# Run the application
CMD ["./enclave-nitro-demo"]