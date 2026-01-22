function MapCanvasMixin:ModifyDataProviderOnUpdate(dataProvider, registered)
	GetOrCreateTableEntry(self, "pendingOnUpdateDataProviders")[dataProvider] = registered;
	self.onUpdateDataProvidersDirty = true;
end
