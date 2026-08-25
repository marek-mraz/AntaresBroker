*** Settings ***
Documentation       Check that one cannot delete an attribute from an entity with invalid/missing ids

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Delete Attributes


*** Variables ***
${status_code}=     404
${filename}=        vehicle-two-datasetid-attributes.jsonld


*** Test Cases ***    ENTITY_ID    ATTRIBUTE_ID    DATASETID
013_03_01 Delete An Attribute When The Entity Id Is Not Known To The System
    ${not_found_entity_id}    speed    urn:ngsi-ld:Property:gpsBxyz123-speed
013_03_02 Delete An Attribute When The Entity Does Not Contain The Target Attribute Id
    ${valid_entity_id}    notFound    ${EMPTY}
013_03_03 Delete An Attribute When The Entity Does Not Contain The Target Attribute With Same datasetId
    ${valid_entity_id}    speed    urn:ngsi-ld:Property:notFound


*** Keywords ***
Delete Attributes
    [Documentation]    Check that one cannot delete an attribute from an entity with invalid/missing ids
    [Tags]    ea-delete    5_6_5
    [Arguments]    ${entity_id}    ${attribute_id}    ${datasetId}
    ${response}=    Delete Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    datasetId=${datasetId}
    ...    deleteAll=false
    Check Response Status Code    ${status_code}    ${response.status_code}

Setup Initial Entity
    ${valid_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${valid_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${valid_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${not_found_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${not_found_entity_id}

Delete Initial Entity
    Delete Entity    ${valid_entity_id}
