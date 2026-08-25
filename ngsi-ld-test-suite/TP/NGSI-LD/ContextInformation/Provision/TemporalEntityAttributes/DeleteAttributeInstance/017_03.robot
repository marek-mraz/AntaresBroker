*** Settings ***
Documentation       Check that one cannot delete an attribute instance in temporal representation of an entity if the EntityId/AttributeId/InstanceId is not right

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Id
Test Teardown       Delete Initial Temporal Representation Of Entity
Test Template       Delete An Attribute Instance


*** Variables ***
${filename}=        vehicle-temporal-representation.jsonld
${status_code}=     404


*** Test Cases ***    TEMPORAL_ENTITY_ID    ATTRIBUTE_ID    INSTANCE_ID
017_03_01 Delete An Attribute Instance In Temporal Representation Of An Entity If The Entity With Given Id Is Not Found
    ${unknown_temporal_entity_id}    speed    ${valid_instanceId}
017_03_02 Delete An Attribute Instance In Temporal Representation Of An Entity If The Target Attribute Is Not Found
    ${temporal_entity_representation_id}    speed2    ${valid_instanceId}
017_03_03 Delete An Attribute Instance In Temporal Representation Of An Entity If The Target Attribute Instance Is Not Found
    ${temporal_entity_representation_id}    speed    urn:ngsi-ld:01234567890123456789


*** Keywords ***
Delete An Attribute Instance
    [Documentation]    Check that one cannot delete an attribute instance in temporal representation of an entity if the EntityId/AttributeId/InstanceId is not found
    [Tags]    tea-instance-delete    5_6_15
    [Arguments]    ${temporal_entity_id}    ${attributeId}    ${instanceId}
    ${response}=    Delete Attribute Instance From Temporal Entity
    ...    ${temporal_entity_id}
    ...    ${attributeId}
    ...    ${instanceId}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    ${status_code}    ${response.status_code}

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
    ${unknown_temporal_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${unknown_temporal_entity_id}

Delete Initial Temporal Representation Of Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
