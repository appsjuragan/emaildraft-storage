
import {
    S3Client,
    CreateBucketCommand,
    PutObjectCommand,
    GetObjectCommand,
    DeleteObjectCommand,
    DeleteBucketCommand
} from "@aws-sdk/client-s3";

const client = new S3Client({
    endpoint: "http://localhost:3000",
    region: "us-east-1",
    credentials: {
        accessKeyId: "objectmail",
        secretAccessKey: "objectmail-secret-key",
    },
    forcePathStyle: true,
});

async function runLargeFileTest() {
    const bucketName = `large-file-test-${Date.now()}`;

    try {
        console.log(`\n\x1b[34mCreating bucket: ${bucketName}\x1b[0m`);
        await client.send(new CreateBucketCommand({ Bucket: bucketName }));

        // 60MB Test
        const size60MB = 60 * 1024 * 1024;
        console.log(`\n\x1b[34mUploading 60MB file...\x1b[0m`);
        const buffer60 = Buffer.alloc(size60MB, 'A');
        await client.send(new PutObjectCommand({
            Bucket: bucketName,
            Key: "60mb-file.bin",
            Body: buffer60,
        }));
        console.log("✅ 60MB Uploaded");

        // 100MB Test
        const size100MB = 100 * 1024 * 1024;
        console.log(`\n\x1b[34mUploading 100MB file...\x1b[0m`);
        const buffer100 = Buffer.alloc(size100MB, 'B');
        await client.send(new PutObjectCommand({
            Bucket: bucketName,
            Key: "100mb-file.bin",
            Body: buffer100,
        }));
        console.log("✅ 100MB Uploaded");

        console.log(`\n\x1b[32m✨ Large file uploads complete! Check server logs for chunk details.\x1b[0m\n`);

        // Optional: verification
        console.log(`\x1b[34mVerifying 60MB file download...\x1b[0m`);
        const { Body: body60 } = await client.send(new GetObjectCommand({
            Bucket: bucketName,
            Key: "60mb-file.bin",
        }));
        const downloaded60 = Buffer.from(await body60?.transformToByteArray() || []);
        if (downloaded60.length !== size60MB) throw new Error("60MB Size mismatch");
        console.log("✅ 60MB Verification successful");

    } catch (error) {
        console.error("\n\x1b[31m❌ TEST FAILED!\x1b[0m");
        console.error(error);
        process.exit(1);
    }
}

runLargeFileTest();
