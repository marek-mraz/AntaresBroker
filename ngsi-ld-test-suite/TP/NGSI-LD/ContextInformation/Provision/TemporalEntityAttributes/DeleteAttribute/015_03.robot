*** Settings ***
Documentation       Check that an error is raised if one deletes a temporal entity with an unknown EntityId/Attribute Id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Id
Test Teardown       Delete Temporal Entity
Test Template       Delete An Attribute


*** Variables ***
${filename}=        vehicle-temporal-representation.jsonld
${status_code}=     404


*** Test Cases ***    ENTITY_ID    ATTRIBUTE_ID
015_03_01 Delete An Attribute To A Temporal Entity If The Entity Id Does Not Exist
    ${unknown_temporal_entity_id}    fuelLevel
015_03_02 Delete An Attribute To A Temporal Entity If The Entity Does Not Contain The Target Attribute
    ${valid_temporal_entity_id}    notExistingAttribute


*** Keywords ***
Delete An Attribute
    [Documentation]    Check that an error is raised if one deletes a temporal entity with an unknown EntityId/Attribute Id
    [Tags]    tea-delete    5_6_13
    [Arguments]    ${entity_id}    ${attribute_id}
    ${response}=    Delete Attribute From Temporal Entity
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Id
    ${valid_temporal_entity_id}=    Generate Random Vehicle Entity Id
    ${unknown_temporal_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${valid_temporal_entity_id}
    Set Test Variable    ${unknown_temporal_entity_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${valid_temporal_entity_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${valid_temporal_entity_id}
