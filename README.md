# ObjectMail - Rust 🦀

**Turn your email inbox into an S3-compatible object storage service.**

ObjectMail is a high-performance, S3-compatible API server written in Rust that leverages your email provider (via IMAP/SMTP) as the underlying storage backend. It cleverly chunks large files into email drafts, deduplicates content to save space, and manages metadata using a local PostgreSQL database.

## 🚀 Features

- **S3 Compatibility**: Supports standard S3 operations including `PutObject`, `GetObject`, `DeleteObject`, `ListObjectsV2`, `CreateBucket`, `DeleteBucket`, and **Multipart Uploads**.
- **Smart Chunking**: Automatically splits large files into configurable chunk sizes (default 18MB) to fit within email provider attachment limits.
- **Deduplication**: Content-addressable storage! Identical chunks are stored only once, saving significant space in your inbox.
- **Recycling Bin**: Deleted objects invoke a smart recycling mechanism where chunks are moved to a system "recycling bin" object instead of being immediately permanently deleted, allowing for future deduplication hits.
- **High Performance**: Built with Rust, Axum, and Tokio for asynchronous, non-blocking I/O.
- **Metadata Management**: Uses PostgreSQL and SeaORM for robust tracking of buckets, objects, and chunk mappings.

##  architektura 🏗️

```mermaid
graph TD
    User[User / S3 Client] -->|S3 API Request| Axum[Axum Web Server]
    
    subgraph "ObjectMail Service"
        Axum -->|Route| S3Router[S3 Router]
        S3Router -->|Auth & Logic| Pipeline[Storage Pipeline]
        
        Pipeline -->|1. Check/Update Metadata| DB[(PostgreSQL)]
        Pipeline -->|2. Chunk & Hash Data| Chunker[Chunker Service]
        
        Chunker -->|3. Check Dedup| DB
        
        Pipeline -->|4a. IF New Chunk| IMAP[IMAP Client (Gmail)]
        Pipeline -->|4b. IF Duplicate| DB
        
        IMAP -->|Upload Draft| EmailProvider[Email Provider\n(e.g., Gmail, GreenMail)]
        
        Pipeline -->|5. Store Chunk Map| DB
    end

    style Axum fill:#f9f,stroke:#333,stroke-width:2px
    style DB fill:#55f,stroke:#333,stroke-width:2px,color:white
    style EmailProvider fill:#dd4,stroke:#333,stroke-width:2px
```

## 🛠️ Setup & Installation

### Prerequisites

- **Rust**: Ensure you have a working Rust environment (latest stable).
- **PostgreSQL**: A running PostgreSQL instance.
- **Email Account**: An IMAP-enabled email account (Gmail specific optimizations are included, but standard IMAP is supported).
  - *Note: For Gmail, use an App Password.*

### Configuration

Create a `.env` file in the root directory:

```env
# Server Configuration
PORT=3000
HOST=0.0.0.0

# Database Configuration
DATABASE_URL=postgres://user:password@localhost:5432/objectmail

# Email Configuration
EMAIL_PROVIDER=gmail
EMAIL_HOST=imap.gmail.com
EMAIL_PORT=993
EMAIL_USER=your-email@gmail.com
EMAIL_PASSWORD=your-app-password
EMAIL_DRAFTS_FOLDER=[Gmail]/Drafts

# Storage Tuning
CHUNK_SIZE_MB=18
```

### Running the Server

1. **Install Dependencies**:
   ```bash
   cargo build
   ```

2. **Run Migrations & Start Server**:
   ```bash
   cargo run
   ```
   *The server will automatically apply database migrations on startup.*

## 📦 Usage

You can use any S3-compatible client. Here are examples using the AWS CLI.

**Configure AWS CLI Profile (Dummy Credentials)**:
```bash
aws configure set aws_access_key_id test --profile objectmail
aws configure set aws_secret_access_key test --profile objectmail
aws configure set region us-east-1 --profile objectmail
```

**Create a Bucket**:
```bash
aws --endpoint-url http://localhost:3000 s3 mb s3://my-backup-bucket --profile objectmail
```

**Upload a File**:
```bash
aws --endpoint-url http://localhost:3000 s3 cp ./large-video.mp4 s3://my-backup-bucket/ --profile objectmail
```

**List Files**:
```bash
aws --endpoint-url http://localhost:3000 s3 ls s3://my-backup-bucket/ --profile objectmail
```

**Download a File**:
```bash
aws --endpoint-url http://localhost:3000 s3 cp s3://my-backup-bucket/large-video.mp4 ./downloaded.mp4 --profile objectmail
```

## 🧪 Testing

The project includes a comprehensive integration test suite written in TypeScript (using Bun) that verifies all core functionality including multipart uploads and deduplication.

To run the tests:

```bash
# Ensure you have Bun installed
bun install

# Run the comprehensive test suite
bun run tests/comprehensive_test.ts
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

This project is licensed under the MIT License.
