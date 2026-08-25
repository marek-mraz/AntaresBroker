*** Settings ***
Documentation       Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Partial Update Attributes


*** Variables ***
${filename}=            vehicle-speed-two-datasetid.jsonld
${status_code}=         404
${default_attr_id}=     speed


*** Test Cases ***    ENTITY_ID    ATTR_ID    FRAGMENT_FILENAME
012_03_01 Partial Update When The Entity Id Is Not Known To The System
    ${not_found_entity_id}    ${default_attr_id}    vehicle-isparked-fragment.jsonld
012_03_02 Partial Update When No Default Instance And No datasetId Specified
    ${valid_entity_id}    ${default_attr_id}    vehicle-speed-no-datasetid-fragment.jsonld
012_03_03 Partial Update When No Instance With The datasetId Specified
    ${valid_entity_id}    ${default_attr_id}    vehicle-speed-unknown-datasetid-fragment.jsonld
012_03_04 Partial Update When The Attribute Name Does Not Exist In The Entity
    ${valid_entity_id}    isParked2    vehicle-isparked-fragment.jsonld


*** Keywords ***
Partial Update Attributes
    [Documentation]    Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not known to the system
    [Tags]    ea-partial-update    5_6_4
    [Arguments]    ${entity_id}    ${attr_id}    ${fragment_filename}
    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attr_id}
    ...    fragment_filename=${fragment_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Initial Entity
    ${valid_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${valid_entity_id}
    ${not_found_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${not_found_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${valid_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${valid_entity_id}
