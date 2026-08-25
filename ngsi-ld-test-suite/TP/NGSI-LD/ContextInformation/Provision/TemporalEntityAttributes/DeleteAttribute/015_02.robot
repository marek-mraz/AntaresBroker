*** Settings ***
Documentation       Check that an error is raised if one deletes an attribute to temporal entity with an unknown/invalid Entity/Attribute Id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Temporal Entity
Test Template       Delete attribute from temporal entity with unknow entity/attribute id


*** Variables ***
${status_code}=     400
${filename}=        vehicle-temporal-representation.jsonld


*** Test Cases ***    ENTITY_ID    ATTRIBUTE_ID
015_02_01 Delete An Attribute To A Temporal Representation Of An Entity With A Missing Entity Id
    ${EMPTY}    speed
015_02_02 Delete An Attribute To A Temporal Representation Of An Entity With An Invalid Entity Id
    invalidId    speed
015_02_03 Delete An Attribute To A Temporal Representation Of An Entity With An Invalid Attribute Id
    ${valid_temporal_entity_id}    invalid(Name


*** Keywords ***
Delete attribute from temporal entity with unknow entity/attribute id
    [Documentation]    Check that an error is raised if one deletes an attribute to temporal entity with an unknown/invalid Entity/Attribute Id
    [Tags]    tea-delete    5_6_13
    [Arguments]    ${entity_id}    ${attribute_id}
    ${response}=    Delete Attribute From Temporal Entity
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Temporal Entity
    ${valid_temporal_entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${valid_temporal_entity_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${valid_temporal_entity_id}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${valid_temporal_entity_id}
