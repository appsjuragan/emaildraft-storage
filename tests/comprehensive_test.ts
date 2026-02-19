
import { 
  S3Client, 
  CreateBucketCommand, 
  PutObjectCommand, 
  GetObjectCommand, 
  ListObjectsV2Command, 
  DeleteObjectCommand, 
  DeleteBucketCommand,
  HeadObjectCommand
} from "@aws-sdk/client-s3";

const client = new S3Client({
  endpoint: "http://localhost:3000",
  region: "us-east-1",
  credentials: {
    accessKeyId: "objectmail",
    secretAccessKey: "objectmail-secret-key",
  },
  forcePathStyle: true, // S3-compatible APIs often need this
});

async function runTest() {
  const bucketName = `test-bucket-${Date.now()}`;
  const key1 = "hello.txt";
  const content1 = "Hello ObjectMail!";
  const key2 = "duplicate.txt";
  const content2 = content1; // Identical content for deduplication test

  try {
    console.log(`\n\x1b[34m[1/7] Creating bucket: ${bucketName}\x1b[0m`);
    await client.send(new CreateBucketCommand({ Bucket: bucketName }));
    console.log("✅ Bucket created");

    console.log(`\n\x1b[34m[2/7] Uploading object: ${key1}\x1b[0m`);
    await client.send(new PutObjectCommand({ 
      Bucket: bucketName, 
      Key: key1, 
      Body: content1,
      ContentType: "text/plain",
      Metadata: { "test-info": "small-file" }
    }));
    console.log("✅ Object uploaded");

    console.log(`\n\x1b[34m[3/7] Verifying GetObject: ${key1}\x1b[0m`);
    const { Body, Metadata } = await client.send(new GetObjectCommand({ 
      Bucket: bucketName, 
      Key: key1 
    }));
    const downloaded = await Body?.transformToString();
    console.log("Downloaded content:", downloaded);
    if (downloaded !== content1) throw new Error("Content mismatch!");
    console.log("Metadata:", Metadata);
    console.log("✅ Content verified");

    console.log(`\n\x1b[34m[4/7] Testing Deduplication: Uploading identical content to ${key2}\x1b[0m`);
    // This should trigger deduplication on the backend
    await client.send(new PutObjectCommand({ 
      Bucket: bucketName, 
      Key: key2, 
      Body: content2 
    }));
    console.log("✅ Duplicate uploaded (check backend logs for 'Deduplication hit')");

    console.log(`\n\x1b[34m[5/7] Listing objects in ${bucketName}\x1b[0m`);
    const list = await client.send(new ListObjectsV2Command({ Bucket: bucketName }));
    console.log("Objects found:", list.Contents?.map(o => o.Key));
    if (list.Contents?.length !== 2) throw new Error(`Expected 2 objects, found ${list.Contents?.length}`);
    console.log("✅ Listing verified");

    console.log(`\n\x1b[34m[6/7] Testing Deletion: Deleting ${key1} (should keep draft for ${key2})\x1b[0m`);
    await client.send(new DeleteObjectCommand({ Bucket: bucketName, Key: key1 }));
    console.log(`Deleted ${key1}`);
    
    // Verify key2 still exists
    await client.send(new HeadObjectCommand({ Bucket: bucketName, Key: key2 }));
    console.log(`✅ ${key2} still exists and is accessible`);

    console.log(`\n\x1b[34m[7/7] Cleaning up: Deleting ${key2} and bucket\x1b[0m`);
    await client.send(new DeleteObjectCommand({ Bucket: bucketName, Key: key2 }));
    await client.send(new DeleteBucketCommand({ Bucket: bucketName }));
    console.log("✅ Cleanup complete");

    console.log("\n\x1b[32m✨ ALL TESTS PASSED SUCCESSFULLY! ✨\x1b[0m\n");
  } catch (error) {
    console.error("\n\x1b[31m❌ TEST FAILED!\x1b[0m");
    console.error(error);
    process.exit(1);
  }
}

runTest();
