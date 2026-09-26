import uvicorn
if __name__=="__main__": uvicorn.run("agi_autopilot.app:app",host="127.0.0.1",port=8120,reload=False)
