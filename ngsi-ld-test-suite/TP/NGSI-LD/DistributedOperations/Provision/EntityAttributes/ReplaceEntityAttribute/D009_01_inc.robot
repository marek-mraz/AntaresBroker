*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker, one is able to replace a target entity and the change is forwarded to the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${entity_attribute_filename}            fragmentEntities/vehicle-speed-two-datasetid-01-fragment.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D009_01_inc Replace Entity Attribute
    [Documentation]    Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Source thanks to an inclusive registration
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_19    6_3_18

    ${attribute_payload}=    Load Entity    ${entity_attribute_filename}    ${entity_id}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    ${old_attribute}=    Get From Dictionary    ${response.json()}    speed
    Set Stub Reply    PUT    /ngsi-ld/v1/entities/${entity_id}/attrs/speed    204

    ${response}=    Replace Attribute Selecting Content Type
    ...    entity_id=${entity_id}
    ...    attr_id=speed
    ...    attribute_fragment=${attribute_payload}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    # ETSI tool bug fixed: the fragment carries no datasetId, so it targets the DEFAULT instance,
    # which this entity lacks (its speed instance has datasetId speed2). Per 5.6.19.4 that is
    # "an error of type ResourceNotFound ... or a partial success if some parts of it succeeded";
    # the forwarded inclusive part succeeded (mock 204), so the overall result is a partial
    # success = 207 Multi-Status per Table 6.7.3.3-2. Replace does NOT append-if-absent
    # (that is Update Attributes 5.6.2.4 semantics only).
    Check Response Status Code    207    ${response.status_code}

    ${stub_count}=    Get Stub Count    PUT    /ngsi-ld/v1/entities/${entity_id}/attrs/speed
    Should Be True    ${stub_count} > 0

    # The static CS mock cannot reflect the replaced value; verify instead that the LOCAL part
    # was left untouched (the local replace legitimately failed - default instance absent).
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    ${new_attribute}=    Get From Dictionary    ${response.json()}    speed
    Should Be Equal    ${old_attribute}[value]    ${new_attribute}[value]


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
