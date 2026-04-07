# Video Compression Assignment (COMP 590)

## Compression Strategy

In this assignment, I aimed to improve the compression strategy already provided in the starter code, by getting the average of the left pixel, the upward pixel(the pixel above) and the 
corresponding pixel in the previous frame.

After computing the prediction, I encoded the difference between the actual pixel and the value that was predicted. 
This process reduces the size of the values being enconded, thus improving compression.

---

## Coding Contexts and Losslessness

To further improve compression, I used 16 arithmetic coding contexts. They were determined by the difference between neighboring pixels:

- If the left and top pixels are similar, a smooth region was formed.
- If they are very different, a detailed region was formed.  

The context is computed as:
context = abs(left - up) / 16

In turn, this allows the encoder to model various image regions separately.

The compression is lossless because each pixel is reconstructed exactly from the predicted value and difference that has been encoded.
The correctness can be verified using '-check-decode' which was provided in the starter code.

---

## Results
The results were tested using `bourne.mp4`. The compression ratio of the starter code was **2.36** while the compression ratio of the method
I implemented is **3.41**

To run, enter this code in the terminal: 
```bash
cargo run -p assgn1 -- -check_decode -count 10
