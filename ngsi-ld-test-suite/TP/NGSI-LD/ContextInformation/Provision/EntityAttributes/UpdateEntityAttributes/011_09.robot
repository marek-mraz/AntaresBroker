*** Settings ***
Documentation       Check that one can delete the default attribute instance using NGSI-LD Null in an Update Attributes operation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Update Attributes


*** Variables ***
${filename}=    building-different-default-attributes-instances-and-types.jsonld


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
011_09_01 Delete A Property
    [Tags]    ea-update    5_6_2    6_6_3_2    since_v1.6.1
    204    ngsild-null/null-property.jsonld    ngsild-null/building-deleted-property-default-instance.jsonld
011_09_02 Delete A Relationship
    [Tags]    ea-update    5_6_2    6_6_3_2    since_v1.6.1
    204    ngsild-null/null-relationship.jsonld    ngsild-null/building-deleted-relationship-default-instance.jsonld
011_09_03 Delete A GeoProperty
    [Tags]    ea-update    5_6_2    6_6_3_2    since_v1.6.1
    204    ngsild-null/null-geoproperty.jsonld    ngsild-null/building-deleted-geoproperty-default-instance.jsonld
011_09_04 Delete A LanguageProperty
    [Tags]    ea-update    5_6_2    6_6_3_2    4_5_18    since_v1.6.1
    204    ngsild-null/null-languageproperty.jsonld    ngsild-null/building-deleted-languageproperty-default-instance.jsonld


*** Keywords ***
Update Attributes
    [Documentation]    Check that one can delete the default attribute instance using NGSI-LD Null in an Update Attributes operation
    [Arguments]
    ...    ${status_code}
    ...    ${fragment_filename}
    ...    ${expectation_filename}
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Body Containing Entity element
    ...    expectation_filename=${expectation_filename}
    ...    entity_id=${entity_id}
    ...    response_body=${response1.json()}

Delete Initial Entity
    Delete Entity    ${entity_id}

Setup Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
