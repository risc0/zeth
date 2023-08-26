// src/VerificationForm.tsx

import React, { useState, ChangeEvent, FormEvent } from 'react';
import { TextField, Button, FormControlLabel, Checkbox, Container, Typography } from '@mui/material';

interface FormData {
    rpc_url: string;
    cache: string;
    network: string;
    block_no: number;
    local_exec: number;
    submit_to_bonsai: boolean;
    verify_bonsai_receipt_uuid: string;
}

const VerificationForm: React.FC = () => {
    const [formData, setFormData] = useState<FormData>({
        rpc_url: '',
        cache: '',
        network: '',
        block_no: 0,
        local_exec: 0,
        submit_to_bonsai: false,
        verify_bonsai_receipt_uuid: ''
    });

    const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
        setFormData({
            ...formData,
            [event.target.name]: event.target.type === 'checkbox' ? event.target.checked : event.target.value
        });
    };

    const handleSubmit = async (event: FormEvent) => {
        event.preventDefault();

        const response = await fetch('http://0.0.0.0:8000/verify', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(formData)
        });

        const data = await response.json();
        console.log(data);
    };

    return (
        <Container maxWidth="sm">
            <Typography variant="h4" component="h1" gutterBottom>
                ZaaS (Zeth As A Service)
            </Typography>
            <form onSubmit={handleSubmit}>
                <TextField variant="filled" label="RPC URL" name="rpc_url" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <TextField variant="filled" label="Cache" name="cache" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <TextField variant="filled" label="Network" name="network" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <TextField variant="filled" label="Block Number" name="block_no" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <TextField variant="filled" label="Local Execution" name="local_exec" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <FormControlLabel
                    control={<Checkbox name="submit_to_bonsai" onChange={handleChange} />}
                    label="Submit to Bonsai"
                />
                <TextField variant="filled" label="Verify Bonsai Receipt UUID" name="verify_bonsai_receipt_uuid" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <Button type="submit" variant="contained" color="primary">Submit</Button>
            </form>
        </Container>
    );
}

export default VerificationForm;