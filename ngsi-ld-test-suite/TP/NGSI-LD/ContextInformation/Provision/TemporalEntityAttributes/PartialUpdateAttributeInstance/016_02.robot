*** Settings ***
Documentation       Check that one cannot modify an attribute instance in temporal representation of an entity if the EntityId/AttributeId/InstanceId is not right

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Id
Test Teardown       Delete Temporal Entity
Test Template       Modify Attribute Instance Temporal Entity


*** Variables ***
${filename}=                vehicle-temporal-representation.jsonld
${fragment_filename}=       vehicle-temporal-modify-attribute-instance-fragment.jsonld


*** Test Cases ***    TEMPORAL_ENTITY_ID    ATTRIBUTE_ID    INSTANCE_ID    EXPECTED_STATUS_CODE
016_02_01 Modify Attribute Instance In Temporal Representation Of An Entity If The Entity Id Is Not Valid
    invalidId    speed    ${valid_instanceId}    400
016_02_02 Modify Attribute Instance In Temporal Representation Of An Entity If The Entity Id Is Not Present
    ${EMPTY}    speed    ${valid_instanceId}    400
016_02_03 Modify Attribute Instance In Temporal Representation Of An Entity If The Instance Id Is Not Valid
    ${temporal_entity_representation_id}    speed    invalidId    400
016_02_04 Modify Attribute Instance In Temporal Representation Of An Entity If The Instance Id Is Not Present
    ${temporal_entity_representation_id}    speed    ${EMPTY}    405
016_02_05 Modify Attribute Instance In Temporal Representation Of An Entity If The Attribute Name Is Not A Valid Name
    ${temporal_entity_representation_id}    invalid(Id    ${valid_instanceId}    400
016_02_06 Modify Attribute Instance In Temporal Representation Of An Entity If The Attribute Name Is Not Present
    ${temporal_entity_representation_id}    ${EMPTY}    ${valid_instanceId}    405


*** Keywords ***
Modify Attribute Instance Temporal Entity
    [Documentation]    Check that one cannot partially modify attribute instance in temporal representation of an entity if the EntityId/AttributeId/InstanceId is not right
    [Tags]    tea-partial-update    5_6_14
    [Arguments]    ${temporal_entity_id}    ${attributeId}    ${instanceId}    ${expected_status_code}
    ${response}=    Modify Attribute Instance From Temporal Entity
    ...    ${temporal_entity_id}
    ...    ${attributeId}
    ...    ${instanceId}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}

Create Id
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    ${temporal_entity_representation_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    options=sysAttrs
    ...    context=${ngsild_test_suite_context}
    ${valid_instanceId}=    Set Variable    ${response.json()['speed'][0]['instanceId']}
    Set Test Variable    ${valid_instanceId}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
